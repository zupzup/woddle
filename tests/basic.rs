use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::time::Duration;
use woddle::{async_trait, Job, JobConfig, JobRunner, RunnerConfig};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug)]
struct MyJobContext {
    pub name: String,
}

#[derive(Clone)]
struct MyJob {
    ctx: MyJobContext,
    config: JobConfig,
}

#[async_trait]
impl Job for MyJob {
    async fn run(&self) {
        COUNTER.fetch_add(1, SeqCst);
        println!("counter");
    }

    fn get_config(&self) -> &JobConfig {
        &self.config
    }
}

#[tokio::test]
async fn test_basic() {
    let job_cfg = JobConfig::new("my_job", "someSyncKey").interval(Duration::from_millis(600));

    let my_job = MyJob {
        ctx: MyJobContext {
            name: "heyo".to_string(),
        },
        config: job_cfg,
    };

    let config = RunnerConfig::default().check_interval(Duration::from_millis(10));
    let job_runner = JobRunner::new(config).add_job(my_job);

    tokio::spawn(async move {
        if let Err(e) = job_runner.start().await {
            log::error!("error: {}", e);
        }
    });

    tokio::time::sleep(Duration::from_millis(1100)).await;

    assert!(COUNTER.load(SeqCst) == 2);
}
