use std::fs;
use crate::SERVER;


#[test]
fn bare_status() -> anyhow::Result<()> {
    SERVER.admin_cmd()
        .arg("create-database").arg("empty")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=empty")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/bare")
        .assert().code(2)
        .stderr(
r###"Detected differences between the database schema and the schema source, in particular:
    CREATE TYPE default::Type1 {
        CREATE OPTIONAL SINGLE PROPERTY field1 -> std::str;
    };
Some migrations are missing, use `edgedb create-migration`
"###);
    Ok(())
}

#[test]
fn initial() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db1/initial/migrations/00002.edgeql")
        .ok();
    fs::remove_file("tests/migrations/db1/initial/migrations/00003.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("create-database").arg("initial")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(3)
        .stderr("Database is empty. While there are 1 migrations \
            on the filesystem. Run `edgedb migrate` to apply\n");
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("create-migration")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(1).stderr("edgedb error: Database must be updated \
            to the last miration on the filesystem for `create-migration`. \
            Run:\n  \
              edgedb migrate\n");
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().success()
        .stderr("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001.edgeql)\n");
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().success()
        .stderr("Database is up to date. \
            Last migration: \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa.\n");
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("create-migration")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(4).stderr("No schema changes detected.\n");
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("create-migration")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(4).stderr("No schema changes detected.\n");

    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("create-migration")
        .arg("--allow-empty")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(0)
        .stderr("Created \
            tests/migrations/db1/initial/migrations/00002.edgeql, \
            id: m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq\n");
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().success()
        .stderr("Applied \
            m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq \
            (00002.edgeql)\n");

    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("create-migration")
        .arg("--allow-empty")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(0).stderr("");
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().success()
        .stderr("Applied \
            m1wrvvw3lycyovtlx4szqm75554g75h5nnbjq3a5qsdncn3oef6nia \
            (00003.edgeql)\n");
    Ok(())
}

#[test]
fn modified1() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db1/modified1/migrations/00002.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("create-database").arg("modified1")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(3)
        .stderr("Database is empty. While there are 1 migrations \
            on the filesystem. Run `edgedb migrate` to apply\n");
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("create-migration")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(1).stderr("edgedb error: Database must be updated \
            to the last miration on the filesystem for `create-migration`. \
            Run:\n  \
              edgedb migrate\n");
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().success()
        .stderr("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001.edgeql)\n");
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(2)
        .stderr(
r###"Detected differences between the database schema and the schema source, in particular:
    CREATE TYPE default::Type2 {
        CREATE OPTIONAL SINGLE PROPERTY field2 -> std::str;
    };
Some migrations are missing, use `edgedb create-migration`
"###);
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("create-migration")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(0).stderr("");
    SERVER.admin_cmd()
        .arg("migration-log")
        .arg("--from-fs")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .arg("--newest-first")
        .arg("--limit=1")
        .assert().code(0)
        .stdout("m12udjjofxzy3nygel35cq4tbz3v56vw7w3d3co6h5hmqhcnodqv3a\n");
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(3)
        .stderr("Database is at migration \
            \"m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa\" \
            while sources contain 1 migrations ahead, \
            starting from \
            \"m12udjjofxzy3nygel35cq4tbz3v56vw7w3d3co6h5hmqhcnodqv3a\"\
            (tests/migrations/db1/modified1/migrations/00002.edgeql)\n");
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().success()
        .stderr("Applied \
            m12udjjofxzy3nygel35cq4tbz3v56vw7w3d3co6h5hmqhcnodqv3a \
            (00002.edgeql)\n");
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(0)
        .stderr("Everything is up to date. Revision \
            \"m12udjjofxzy3nygel35cq4tbz3v56vw7w3d3co6h5hmqhcnodqv3a\"\n");
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().success()
        .stderr("Database is up to date. \
            Last migration: \
            m12udjjofxzy3nygel35cq4tbz3v56vw7w3d3co6h5hmqhcnodqv3a.\n");
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("create-migration")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(4).stderr("No schema changes detected.\n");
    SERVER.admin_cmd()
        .arg("migration-log")
        .arg("--from-fs")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .arg("--newest-first")
        .assert().code(0)
        .stdout("\
            m12udjjofxzy3nygel35cq4tbz3v56vw7w3d3co6h5hmqhcnodqv3a\n\
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa\n\
        ");
    SERVER.admin_cmd()
        .arg("migration-log")
        .arg("--from-fs")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(0)
        .stdout("\
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa\n\
            m12udjjofxzy3nygel35cq4tbz3v56vw7w3d3co6h5hmqhcnodqv3a\n\
        ");
    Ok(())
}

#[test]
fn error() -> anyhow::Result<()> {
    SERVER.admin_cmd()
        .arg("create-database").arg("empty_err")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=empty_err")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/error")
        .env("NO_COLOR", "1")
        .assert().code(1)
        .stderr(
r###"error: Unexpected 'create'
  ┌─ tests/migrations/db1/error/bad.esdl:3:9
  │
3 │         create property text -> str;
  │         ^^^^^^^ error

edgedb error: cannot proceed until .esdl files are fixed
"###);
    Ok(())
}

#[test]
fn modified2_interactive() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db1/modified2/migrations/00002.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("create-database").arg("modified2")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=modified2")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified2")
        .assert().success()
        .stderr("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001.edgeql)\n");

    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.arg("--database=modified2");
        cmd.arg("create-migration");
        cmd.arg("--schema-dir=tests/migrations/db1/modified2");
    });
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("y\n").unwrap();
    cmd.exp_string("Created \
        tests/migrations/db1/modified2/migrations/00002.edgeql, \
        id: m12udjjofxzy3nygel35cq4tbz3v56vw7w3d3co6h5hmqhcnodqv3a").unwrap();

    SERVER.admin_cmd()
        .arg("--database=modified2")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified2")
        .assert().success()
        .stderr("Applied \
            m12udjjofxzy3nygel35cq4tbz3v56vw7w3d3co6h5hmqhcnodqv3a \
            (00002.edgeql)\n");
    SERVER.admin_cmd()
        .arg("--database=modified2")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/modified2")
        .assert().success()
        .stderr("Database is up to date. \
            Last migration: \
            m12udjjofxzy3nygel35cq4tbz3v56vw7w3d3co6h5hmqhcnodqv3a.\n");
    SERVER.admin_cmd()
        .arg("--database=modified2")
        .arg("create-migration")
        .arg("--schema-dir=tests/migrations/db1/modified2")
        .assert().code(4).stderr("No schema changes detected.\n");
    Ok(())
}

#[test]
fn modified3_interactive() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db1/modified3/migrations/00002.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("create-database").arg("modified3")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=modified3")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified3")
        .assert().success()
        .stderr("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001.edgeql)\n");

    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.arg("--database=modified3");
        cmd.arg("create-migration");
        cmd.arg("--schema-dir=tests/migrations/db1/modified3");
    });
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes\n").unwrap();
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes\n").unwrap();
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("back\n").unwrap();
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes\n").unwrap();
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes\n").unwrap();
    cmd.exp_string("Created").unwrap();

    SERVER.admin_cmd()
        .arg("--database=modified3")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified3")
        .assert().success();  // revision can be different because of order
    SERVER.admin_cmd()
        .arg("--database=modified3")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/modified3")
        .assert().success();  // revision can be different because of order
    SERVER.admin_cmd()
        .arg("--database=modified3")
        .arg("create-migration")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/modified3")
        .assert().code(4).stderr("No schema changes detected.\n");
    Ok(())
}
