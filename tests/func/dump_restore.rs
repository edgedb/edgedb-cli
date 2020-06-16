use crate::SERVER;

#[test]
fn dump_restore_cycle() {
    println!("before");
    SERVER.admin_cmd().arg("create-database").arg("dump_01")
        .assert().success();
    println!("dbcreated");
    SERVER.database_cmd("dump_01").arg("query")
        .arg("CREATE TYPE Hello { CREATE REQUIRED PROPERTY name -> str; }")
        .arg("INSERT Hello { name := 'world' }")
        .assert().success();
    println!("Created");
    SERVER.database_cmd("dump_01").arg("dump").arg("dump_01.dump")
        .assert().success();
    println!("dumped");
    SERVER.admin_cmd().arg("create-database").arg("restore_01")
        .assert().success();
    println!("created2");
    SERVER.database_cmd("restore_01").arg("restore").arg("dump_01.dump")
        .assert().success();
    println!("restored");
    SERVER.database_cmd("restore_01").arg("query")
        .arg("SELECT Hello.name")
        .assert().success()
        .stdout("\"world\"\n");
    println!("query");
}
