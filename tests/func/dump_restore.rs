use test_utils::server::ServerInstance;

use crate::{ServerGuard, SERVER};

#[test]
fn dump_restore_cycle() {
    std::fs::create_dir_all("./tmp").expect("can create directory");
    println!("before");
    SERVER
        .admin_cmd()
        .arg("database")
        .arg("create")
        .arg("dump_01")
        .assert()
        .success();
    println!("dbcreated");
    SERVER
        .database_cmd("dump_01")
        .arg("query")
        .arg("CREATE TYPE Hello { CREATE REQUIRED PROPERTY name -> str; }")
        .arg("INSERT Hello { name := 'world' }")
        .assert()
        .success();
    println!("Created");
    SERVER
        .database_cmd("dump_01")
        .arg("dump")
        .arg("./tmp/dump_01.dump")
        .assert()
        .success();
    println!("dumped");
    SERVER
        .admin_cmd()
        .arg("database")
        .arg("create")
        .arg("restore_01")
        .assert()
        .success();
    println!("created2");
    SERVER
        .database_cmd("restore_01")
        .arg("restore")
        .arg("./tmp/dump_01.dump")
        .assert()
        .success();
    println!("restored");
    SERVER
        .database_cmd("restore_01")
        .arg("query")
        .arg("SELECT Hello.name")
        .assert()
        .success()
        .stdout("\"world\"\n");
    println!("query");
}

#[test]
fn dump_all_without_a_format() {
    SERVER
        .admin_cmd()
        .arg("dump")
        .arg("--all")
        .arg("dump01-dir")
        .assert()
        .code(1);
}

#[test]
fn dump_restore_all() {
    println!("before");
    SERVER
        .admin_cmd()
        .arg("database")
        .arg("create")
        .arg("dump_02")
        .assert()
        .success();
    println!("dbcreated");
    SERVER
        .database_cmd("dump_02")
        .arg("query")
        .arg("CREATE TYPE Hello { CREATE REQUIRED PROPERTY name -> str; }")
        .arg("INSERT Hello { name := 'world' }")
        .assert()
        .success();
    println!("Created");
    SERVER
        .admin_cmd()
        .arg("dump")
        .arg("--all")
        .arg("--format=dir")
        .arg("./tmp/dump_02")
        .assert()
        .success();
    println!("dumped");

    let new_instance = ServerGuard(ServerInstance::start());
    println!("new instance started");
    new_instance
        .admin_cmd()
        .arg("restore")
        .arg("--all")
        .arg("./tmp/dump_02")
        .assert()
        .success();
    println!("restored");

    new_instance
        .database_cmd("dump_02")
        .arg("query")
        .arg("SELECT Hello.name")
        .assert()
        .success()
        .stdout("\"world\"\n");
    new_instance.0.stop();
    println!("query");
}

#[!cfg(target_os = "windows")]
#[test]
fn dump_restore_dev() {
    println!("before");
    SERVER
        .admin_cmd()
        .arg("database")
        .arg("create")
        .arg("dump_03")
        .assert()
        .success();
    println!("dbcreated");
    SERVER
        .database_cmd("dump_03")
        .arg("query")
        .arg("CREATE TYPE Hello { CREATE REQUIRED PROPERTY name -> str; }")
        .arg("INSERT Hello { name := 'world' }")
        .assert()
        .success();
    println!("Created");
    let dumped_data = SERVER
        .admin_cmd()
        .arg("dump")
        .arg("/dev/stdout")
        .assert()
        .success()
        .get_output();
    println!("dumped");
    SERVER
        .admin_cmd()
        .arg("database")
        .arg("create")
        .arg("restore_03")
        .assert()
        .success();
    println!("created2");
    SERVER
        .database_cmd("restore_03")
        .arg("restore")
        .arg("/dev/stdin")
        .write_stdin(&dumped_data.stdout)
        .assert()
        .success();
    println!("restored");
    SERVER
        .database_cmd("restore_03")
        .arg("query")
        .arg("SELECT Hello.name")
        .assert()
        .success()
        .stdout("\"world\"\n");
    println!("query");
}
