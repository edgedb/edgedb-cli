use std::error::Error;
use async_std::task;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    task::block_on(async {
        let pool = edgedb_client::connect().await?;
        /*
        pool.execute("START MIGRATION TO {}", &()).await?;
        let val = pool.query_single::<edgedb_client::model::Json, _>(
            "DESCRIBE CURRENT MIGRATION AS JSON", &()).await?;
        dbg!(val);
        let val = pool.query_single_json(
            "DESCRIBE CURRENT MIGRATION AS JSON", &()).await?;
        dbg!(val);
        */
        let val = pool.query_single::<edgedb_client::model::Json, _>(
            "SELECT <json>{a := 1, b := 2}", &()).await?;
        dbg!(val);
        let val = pool.query_single_json(
            "SELECT <json>{a := 1, b := 2}", &()).await?;
        dbg!(val);
        Ok::<_, Box<dyn Error>>(())
    })?;
    Ok(())
}
