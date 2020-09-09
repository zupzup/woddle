use crate::db::DBError;
use mobc::Pool;
use mobc_postgres::{tokio_postgres, PgConnectionManager};
use tokio_postgres::{Client, NoTls};

pub(crate) type DBPool = Pool<PgConnectionManager<NoTls>>;

// Gets a database connection from the connection pool
pub(crate) async fn get_con(pool: &DBPool) -> Result<Client, DBError> {
    let con = pool.get().await.map_err(DBError::PoolError)?;
    Ok(con.into_inner())
}
