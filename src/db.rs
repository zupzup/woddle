use crate::{config::RunnerConfig, BoxedJob};
use chrono::prelude::*;
use std::fmt;
use tokio_postgres::{row::Row, Client, NoTls};

/// Database error cases
#[derive(Debug)]
pub enum DBError {
    ConnectionError(tokio_postgres::Error),
    CreateJobError(tokio_postgres::Error),
    CreateTableError(tokio_postgres::Error),
    UpdateJobError(tokio_postgres::Error),
    #[cfg(feature = "pool-mobc")]
    PoolError(mobc::Error<tokio_postgres::Error>),
}

impl std::error::Error for DBError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            DBError::ConnectionError(ref err) => Some(err),
            DBError::CreateJobError(ref err) => Some(err),
            DBError::CreateTableError(ref err) => Some(err),
            DBError::UpdateJobError(ref err) => Some(err),
            #[cfg(feature = "pool-mobc")]
            DBError::PoolError(ref err) => Some(err),
        }
    }
}

impl fmt::Display for DBError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DBError::ConnectionError(ref e) => write!(f, "error connecting to the database: {}", e),
            DBError::CreateJobError(ref e) => {
                write!(f, "error creating a job in the database: {}", e)
            }
            DBError::CreateTableError(ref e) => {
                write!(f, "error during creating sync table: {}", e)
            }
            DBError::UpdateJobError(ref e) => write!(f, "error during db update: {}", e),
            #[cfg(feature = "pool-mobc")]
            DBError::PoolError(ref e) => write!(f, "error getting connection from the pool: {}", e),
        }
    }
}

const ASSERT_JOBS_TABLE_QUERY: &str = r#"CREATE TABLE IF NOT EXISTS woddle_jobs (
        sync_key VARCHAR(255) PRIMARY KEY NOT NULL, last_run timestamp with time zone);"#;
const INSERT_JOB_QUERY: &str =
    "INSERT INTO woddle_jobs (sync_key) VALUES ($1) ON CONFLICT DO NOTHING";
const LOCK_JOB_QUERY: &str = "SELECT * FROM woddle_jobs WHERE sync_key = $1 FOR UPDATE";
const UPDATE_JOB_QUERY: &str =
    "UPDATE woddle_jobs set last_run = (now() at time zone 'utc') WHERE sync_key = $1";

#[derive(Debug)]
struct DBJob {
    pub sync_key: String,
    pub last_run: Option<DateTime<Utc>>,
}

impl DBJob {
    pub fn from_row(row: &Row) -> Self {
        Self {
            sync_key: row.get(0),
            last_run: row.get(1),
        }
    }
}

// Asserts the woddle_jobs table into existence
pub(crate) async fn create_tables(client: &Client) -> Result<(), DBError> {
    client
        .execute(ASSERT_JOBS_TABLE_QUERY, &[])
        .await
        .map_err(DBError::CreateTableError)?;
    Ok(())
}

// Inserts a job into the woddle_jobs table, if doesn't exist yet
pub(crate) async fn insert_job(client: &Client, job: &BoxedJob) -> Result<(), DBError> {
    client
        .execute(INSERT_JOB_QUERY, &[&job.get_config().name])
        .await
        .map_err(DBError::CreateJobError)?;
    Ok(())
}

// Checks if the given job is ready to run and, if so, set `last_run` to `now()`.
// This function locks the row for the given job and updates it, if necessary, which
// ensures, that only one instance of woddle can access it at any time, avoiding races.
//
// Returns `true` if the job was ready and had it's `last_run` updated to `now()`
pub(crate) async fn update_job_if_ready(
    client: &mut Client,
    job: &BoxedJob,
) -> Result<bool, DBError> {
    let tx = client
        .transaction()
        .await
        .map_err(DBError::UpdateJobError)?;

    let db_job = DBJob::from_row(
        &tx.query_one(LOCK_JOB_QUERY, &[&job.get_config().name])
            .await
            .map_err(DBError::UpdateJobError)?,
    );

    if is_ready_to_run(job, &db_job.last_run) {
        let updated_rows = tx
            .execute(UPDATE_JOB_QUERY, &[&job.get_config().name])
            .await
            .map_err(DBError::UpdateJobError)?;

        tx.commit().await.map_err(DBError::UpdateJobError)?;

        return Ok(updated_rows == 1);
    }

    tx.commit().await.map_err(DBError::UpdateJobError)?;
    Ok(false)
}

// Checks if a job is ready to run, either via it's cron, or interval config
fn is_ready_to_run(job: &BoxedJob, last_run: &Option<DateTime<Utc>>) -> bool {
    let cfg = job.get_config();
    let now = Utc::now();

    match cfg.interval {
        Some(interval) => {
            match last_run {
                Some(lr) => {
                    match chrono::Duration::from_std(interval) {
                        Ok(dur) => match lr.checked_add_signed(dur) {
                            Some(new_run_time) => now.gt(&new_run_time),
                            None => {
                                log::error!("could add durations - job {} will not run", cfg.name);
                                false
                            }
                        },
                        Err(e) => {
                            log::error!("could not convert duration for job {} because of {}, job will not run", cfg.name, e);
                            false
                        }
                    }
                }
                None => true,
            }
        }
        None => match cfg.cron {
            Some(ref cron) => match last_run {
                Some(lr) => match cron.after(lr).take(1).next() {
                    Some(upcoming) => upcoming.lt(&now),
                    None => {
                        log::warn!(
                            "cron expression for job {} could not produce an upcoming run time",
                            cfg.name
                        );
                        false
                    }
                },
                None => true,
            },
            None => unreachable!(
                "can't get here, since running jobs with neither cron, nor interval fails earlier"
            ),
        },
    }
}

// Initiates a database connection
#[cfg(not(feature = "pool-mobc"))]
pub(crate) async fn get_con(cfg: &RunnerConfig) -> Result<Client, DBError> {
    get_new_con(cfg).await
}

// Initiates a pooled database connection
#[cfg(feature = "pool-mobc")]
pub(crate) async fn get_con(cfg: &RunnerConfig) -> Result<Client, DBError> {
    use crate::pool;
    match cfg.pool {
        Some(ref db_pool) => pool::get_con(db_pool).await,
        None => get_new_con(cfg).await,
    }
}

async fn get_new_con(cfg: &RunnerConfig) -> Result<Client, DBError> {
    let (client, connection) = cfg
        .db
        .connect(NoTls)
        .await
        .map_err(DBError::ConnectionError)?;

    tokio::spawn(connection);
    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{async_trait, Job, JobConfig};
    use std::time::Duration;

    #[derive(Clone)]
    struct TestJob {
        cfg: JobConfig,
    }

    #[async_trait]
    impl Job for TestJob {
        async fn run(&self) {}

        fn get_config(&self) -> &JobConfig {
            &self.cfg
        }
    }

    #[test]
    fn test_is_ready_to_run_interval_no_last_run() {
        let job = Box::new(TestJob {
            cfg: JobConfig::new("test", "test").interval(Duration::from_secs(1)),
        }) as BoxedJob;
        assert!(is_ready_to_run(&job, &None));
    }

    #[test]
    fn test_is_ready_to_run_cron_no_last_run() {
        let job = Box::new(TestJob {
            cfg: JobConfig::new("test", "test").cron("* * * * * * *"),
        }) as BoxedJob;
        assert!(is_ready_to_run(&job, &None));
    }

    #[test]
    fn test_is_ready_to_run_cron_not_yet() {
        let job = Box::new(TestJob {
            cfg: JobConfig::new("test", "test").cron("0 30 9 1 * * 2100"),
        }) as BoxedJob;
        assert!(!is_ready_to_run(&job, &Some(Utc::now())));
    }

    #[test]
    fn test_is_ready_to_run_interval_not_long_enough() {
        let job = Box::new(TestJob {
            cfg: JobConfig::new("test", "test").interval(Duration::from_secs(10000)),
        }) as BoxedJob;
        assert!(!is_ready_to_run(&job, &Some(Utc::now())));
    }

    #[test]
    fn test_is_ready_to_run_interval_ready() {
        let job = Box::new(TestJob {
            cfg: JobConfig::new("test", "test").interval(Duration::from_secs(20)),
        }) as BoxedJob;
        assert!(is_ready_to_run(
            &job,
            &Some(
                Utc::now()
                    .checked_sub_signed(chrono::Duration::minutes(60))
                    .unwrap()
            )
        ));
    }

    #[test]
    fn test_is_ready_to_run_cron_ready() {
        let job = Box::new(TestJob {
            cfg: JobConfig::new("test", "test").cron("0 30 9 1 * * 2018"),
        }) as BoxedJob;
        assert!(!is_ready_to_run(&job, &Some(Utc::now())));
    }
}
