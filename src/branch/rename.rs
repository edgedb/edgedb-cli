use crate::branch::context::Context;
use crate::branch::option::Rename;
use crate::connect::Connection;
use crate::print;

pub async fn main(options: &Rename, _context: &Context, connection: &mut Connection) -> anyhow::Result<()> {
    let status = connection.execute(
        &format!("alter branch {} rename to {}", options.old_name, options.new_name),
        &()
    ).await?;

    print::completion(status);

    Ok(())
}