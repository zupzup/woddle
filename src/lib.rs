//! An async, synchronized, database-backed Rust job scheduler
//!
//! This library provides an async job runner, which can run user-defined jobs in an interval, or
//! based on a cron schedule.
//!
//! Also, the library automatically synchronizes multiple instances of the runner via PostgreSQL.
//! This is important to ensure, that a job is only run once for each interval, or schedule.
//!
//! A `Job` in this library can be created by implementing the `Job` trait. There the user can
//! define a custom `run` function, which is executed for each interval, or schedule of the job.
//!
//! This interval, as well as other relevant metadata, needs to be configured using a `JobConfig`
//! for each job.
//!
//! Then, once all your jobs are defined, you can create a `JobRunner`. This is the main mechanism
//! underlying this scheduling library. It will check, at a user-defined interval, if a job needs
//! to run, or not.
//!
//! This `JobRunner` is configured using the `RunnerConfig`, where the user can define database
//! configuration, as well as an initial delay and the interval for checking for job runs.
//!
//! Once everything is configured, you can run the `JobRunner` and, if it doesn't return an error
//! during job validation, it will run forever, scheduling and running your jobs asynchronously
//! using Tokio.
#![cfg_attr(feature = "docs", feature(doc_cfg))]
#![warn(missing_docs)]
mod config;

pub use async_trait::async_trait;
pub use config::JobConfig;
pub use config::RunnerConfig;
use futures::future::join_all;
use log::{error, info};
use std::fmt;
use std::sync::Arc;

mod db;

#[cfg(feature = "pool-mobc")]
mod pool;

type BoxedJob = Box<dyn Job + Send + Sync>;

/// The error type returned by methods in this crate
#[derive(Debug)]
pub enum Error {
    /// A database error
    DBError(db::DBError),
    /// An error parsing the database configuration
    DBConfigError(tokio_postgres::Error),
    /// An error indicating an invalid job, with neither `cron`, nor `interval` set
    InvalidJobError,
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::DBError(ref e) => write!(f, "db error: {}", e),
            Error::DBConfigError(ref e) => write!(f, "db configuration error: {}", e),
            Error::InvalidJobError => write!(
                f,
                "invalid job found - check if all jobs have interval or cron set"
            ),
        }
    }
}

#[async_trait]
/// A trait for implementing a woddle job
///
/// Example implementation:
///
/// ```ignore
/// use std::time::Duration;
/// use crate::{JobConfig, Job, async_trait};
///
/// #[derive(Clone)]
/// struct MyJob {
///     config: JobConfig,
/// }
///
/// #[async_trait]
/// impl Job for MyJob {
///     async fn run(&self) {
///         log::info!("running my job!");
///     }
///
///     fn get_config(&self) -> &JobConfig {
///         &self.config
///     }
/// }
///
/// fn main() {
///     let job_cfg = JobConfig::new("my_job", "someSyncKey").interval(Duration::from_secs(5));

///     let my_job = MyJob {
///         config: job_cfg,
///     };
/// }
/// ```
pub trait Job: JobClone {
    /// Runs the job
    ///
    /// This is an async function, so if you plan to do long-running, blocking operations, you
    /// should spawn them on [Tokio's Blocking Threadpool](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).
    ///
    /// You need the `blocking` feature to be active, for this to work.
    ///
    /// Otherwise, you might block the scheduler threads, slowing down your whole application.
    async fn run(&self);
    /// Exposes the configuration of the job
    fn get_config(&self) -> &JobConfig;
}

#[doc(hidden)]
pub trait JobClone {
    fn box_clone(&self) -> BoxedJob;
}

impl<T> JobClone for T
where
    T: 'static + Job + Clone + Send + Sync,
{
    fn box_clone(&self) -> BoxedJob {
        Box::new((*self).clone())
    }
}

impl Clone for Box<dyn Job> {
    fn clone(&self) -> Box<dyn Job> {
        self.box_clone()
    }
}

/// The runner, which holds the jobs and runner configuration
pub struct JobRunner {
    jobs: Vec<BoxedJob>,
    config: RunnerConfig,
}

impl JobRunner {
    /// Creates a new runner based on the given RunnerConfig
    pub fn new(config: RunnerConfig) -> Self {
        Self {
            config,
            jobs: Vec::new(),
        }
    }

    /// Adds a job to the Runner
    pub fn add_job(mut self, job: impl Job + Send + Sync + Clone + 'static) -> Self {
        self.jobs.push(Box::new(job) as BoxedJob);
        self
    }

    /// Starts the runner
    ///
    /// This will:
    ///
    /// * Validate the added jobs
    /// * Initialize the database state, creating the `woddle_jobs` table
    /// * Announce all registered jobs with their timers
    /// * Start checking and running jobs
    pub async fn start(self) -> Result<(), Error> {
        self.validate()?;
        self.initialize().await?;
        self.announce_jobs();

        if let Some(initial_delay) = self.config.initial_delay {
            tokio::time::sleep(initial_delay).await;
        }

        let mut job_interval = tokio::time::interval(self.config.check_interval);
        let jobs = Arc::new(&self.jobs);
        loop {
            job_interval.tick().await;
            self.check_and_run_jobs(jobs.clone()).await;
        }
    }

    // Validates all jobs
    fn validate(&self) -> Result<(), Error> {
        for job in &self.jobs {
            let cfg = job.get_config();
            if cfg.interval.is_none() && cfg.cron.is_none() {
                return Err(Error::InvalidJobError);
            }
        }
        Ok(())
    }

    // Asserts that the woddle_jobs table is there and insert all new jobs
    async fn initialize(&self) -> Result<(), Error> {
        let con = db::get_con(&self.config).await.map_err(Error::DBError)?;
        db::create_tables(&con).await.map_err(Error::DBError)?;
        for j in self.jobs.iter() {
            db::insert_job(&con, &j).await.map_err(Error::DBError)?;
        }
        Ok(())
    }

    // Logs an announcement for all registered jobs
    fn announce_jobs(&self) {
        for job in &self.jobs {
            match job.get_config().interval {
                Some(interval) => {
                    info!(
                        "job '{}' with interval: {:?} registered successfully",
                        job.get_config().name,
                        interval
                    );
                }
                None => match job.get_config().cron_str {
                    Some(ref cron) => {
                        info!(
                            "job '{}' with cron-schedule: {:?} registered successfully",
                            job.get_config().name,
                            cron
                        );
                    }
                    None => unreachable!("can't get here, since running a job with neither cron, nor interval fails earlier"),
                },
            }
        }
    }

    // Checks and runs, if necessary, all jobs concurrently
    async fn check_and_run_jobs(&self, jobs: Arc<&Vec<BoxedJob>>) {
        let job_futures = jobs
            .iter()
            .map(|job| {
                let j = job.box_clone();
                self.check_and_run_job(j)
            })
            .collect::<Vec<_>>();
        join_all(job_futures).await;
    }

    // Checks and runs a single [Job](crate::Job)
    //
    // Connects to the database, checks if the given job should be run again and if so, sets the
    // `last_run` of the job to `now()` and executes the job.
    async fn check_and_run_job(&self, job: BoxedJob) -> Result<(), Error> {
        let mut con = db::get_con(&self.config).await.map_err(|e| {
            error!("error checking job {}, {}", job.get_config().name, e);
            Error::DBError(e)
        })?;

        let should_run_job = db::update_job_if_ready(&mut con, &job).await.map_err(|e| {
            error!("error checking job {}, {}", job.get_config().name, e);
            Error::DBError(e)
        })?;

        if should_run_job {
            tokio::spawn(async move {
                job.run().await;
            });
        }

        Ok(())
    }
}
