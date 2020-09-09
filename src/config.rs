use crate::Error;
use cron::Schedule;
use std::str::FromStr;
use std::time::Duration;
use tokio_postgres::config::Config;

#[derive(Default, Clone)]
/// Configuration for a job
pub struct JobConfig {
    /// An arbitrary identifier used for logging
    pub name: String,
    /// A unique identifier used within the database state
    pub(crate) sync_key: String,
    /// The interval the job should run at (e.g.: 10s, `std::time::Duration::from_secs(10)`)
    pub(crate) interval: Option<Duration>,
    /// The Quartz cron expression to use, for defining job run times (e.g.: `"* 0 0 ? * * *"`)
    pub(crate) cron: Option<Schedule>,
    pub(crate) cron_str: Option<String>,
}

/// Configuration for a single woddle job
///
/// Initialized with the name and sync_key of the job.
///
/// The name is an arbitrary identifier for the job.
/// The sync_key is a unique identifier, which is used within the database.
///
/// For a job to be valid, either the `interval`, or the `cron` configuration need to be set.
///
/// If neither are set, the job_runner will exit with an Error.
impl JobConfig {
    /// Create a new job with an arbitrary name and a unique sync key
    ///
    /// After creating a job, you need to set either `interval`, or `cron` for the job to be valid
    pub fn new(name: &str, sync_key: &str) -> Self {
        Self {
            name: name.to_owned(),
            sync_key: sync_key.to_owned(),
            ..Default::default()
        }
    }

    /// Sets the interval a job should be run at
    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval = Some(interval);
        self
    }

    /// Creates a cron schedule from the given Quartz cron expression
    ///
    /// Example: `"* 0 0 ? * * *"`
    ///
    /// # Panics
    ///
    /// Panics if the expression is not valid
    pub fn cron(mut self, expression: &str) -> Self {
        self.cron = Some(Schedule::from_str(expression).expect("invalid cron expression"));
        self.cron_str = Some(expression.to_owned());
        self
    }
}

/// Configuration for the woddle job runner.
///
/// Holds the database configuration and the interval at which to check
/// for new job runs.
///
/// Database configuration defaults to `host=localhost user=postgres port=5432`
///
/// Check Interval defaults to 60 seconds. This means, that every 60 seconds, the system checks, if
/// a job should be run.
///
/// Initial delay defaults to 0 seconds. This means, that the runner starts immediately
#[derive(Clone)]
pub struct RunnerConfig {
    /// Interval for checking and running jobs
    pub(crate) check_interval: Duration,
    /// Amounts of time to wait, before starting to check and run jobs after startup
    pub(crate) initial_delay: Option<Duration>,
    /// Database configuration, based on [Config](tokio_postgres::config::Config)
    pub(crate) db: Config,
    #[cfg(feature = "pool-mobc")]
    /// Optional [Database pool](mobc_postgres), if the `pool-mobc` feature is enabled
    pub(crate) pool: Option<crate::pool::DBPool>,
}

impl RunnerConfig {
    /// Creates a new RunnerConfig with the given connection String
    ///
    /// Example: `host=localhost user=postgres port=5432 password=postgres`
    ///
    /// Anything, which can be used to create a [Config](https://docs.rs/tokio-postgres/0.5/tokio_postgres/config/struct.Config.html) is valid.
    pub fn new(db_config: &str) -> Result<Self, Error> {
        let res = Self {
            db: Config::from_str(db_config).map_err(Error::DBConfigError)?,
            ..Default::default()
        };
        Ok(res)
    }

    /// Sets the interval to check for job runs to the given Duration
    pub fn check_interval(mut self, check_interval: Duration) -> Self {
        self.check_interval = check_interval;
        self
    }

    /// Sets the initial delay, before checking and running jobs to the given Duration
    pub fn initial_delay(mut self, initial_delay: Duration) -> Self {
        self.initial_delay = Some(initial_delay);
        self
    }

    #[cfg(feature = "pool-mobc")]
    /// Sets the connection pool, if the `pool-mobc` feature is enabled
    pub fn pool(mut self, pool: crate::pool::DBPool) -> Self {
        self.pool = Some(pool);
        self
    }
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(60),
            initial_delay: None,
            #[cfg(feature = "pool-mobc")]
            pool: None,
            db: Config::from_str("host=localhost user=postgres port=5432")
                .expect("default config is valid"),
        }
    }
}
