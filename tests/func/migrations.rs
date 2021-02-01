use std::fs;
use crate::SERVER;
use predicates::str::ends_with;


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
        .stderr(ends_with(
r###"Detected differences between the database schema and the schema source, in particular:
    CREATE TYPE default::Type1 {
        CREATE PROPERTY field1 -> std::str;
    };
Some migrations are missing, use `edgedb create-migration`
"###));
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
        .stderr(ends_with("Database is empty. While there are 1 migrations \
            on the filesystem. Run `edgedb migrate` to apply\n"));
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("create-migration")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(1).stderr(ends_with(
            "edgedb error: Database must be updated \
            to the last migration on the filesystem for `create-migration`. \
            Run:\n  \
              edgedb migrate\n"));
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().success()
        .stderr(ends_with("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001.edgeql)\n"));
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().success()
        .stderr(ends_with("Database is up to date. \
            Last migration: \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa.\n"));
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("create-migration")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(4).stderr(ends_with("No schema changes detected.\n"));
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("create-migration")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(4).stderr(ends_with("No schema changes detected.\n"));

    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("create-migration")
        .arg("--allow-empty")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(0)
        .stderr(ends_with("Created \
            tests/migrations/db1/initial/migrations/00002.edgeql, \
            id: m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq\n"));
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().success()
        .stderr(ends_with("Applied \
            m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq \
            (00002.edgeql)\n"));

    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("create-migration")
        .arg("--allow-empty")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(0);
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().success()
        .stderr(ends_with("Applied \
            m1wrvvw3lycyovtlx4szqm75554g75h5nnbjq3a5qsdncn3oef6nia \
            (00003.edgeql)\n"));
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
        .stderr(ends_with("Database is empty. While there are 1 migrations \
            on the filesystem. Run `edgedb migrate` to apply\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("create-migration")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(1).stderr(ends_with(
            "edgedb error: Database must be updated \
            to the last migration on the filesystem for `create-migration`. \
            Run:\n  \
              edgedb migrate\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().success()
        .stderr(ends_with("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001.edgeql)\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(2)
        .stderr(ends_with(
r###"Detected differences between the database schema and the schema source, in particular:
    CREATE TYPE default::Type2 {
        CREATE PROPERTY field2 -> std::str;
    };
Some migrations are missing, use `edgedb create-migration`
"###));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("create-migration")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(0);
    SERVER.admin_cmd()
        .arg("migration-log")
        .arg("--from-fs")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .arg("--newest-first")
        .arg("--limit=1")
        .assert().code(0)
        .stdout("m1caxjxlggy5xv63isfp5oxdbucx35efhgevxdklvlcgjgpdus3j3q\n");
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(3)
        .stderr(ends_with("Database is at migration \
            \"m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa\" \
            while sources contain 1 migrations ahead, \
            starting from \
            \"m1caxjxlggy5xv63isfp5oxdbucx35efhgevxdklvlcgjgpdus3j3q\"\
            (tests/migrations/db1/modified1/migrations/00002.edgeql)\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().success()
        .stderr(ends_with("Applied \
            m1caxjxlggy5xv63isfp5oxdbucx35efhgevxdklvlcgjgpdus3j3q \
            (00002.edgeql)\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(0)
        .stderr(ends_with("Everything is up to date. Revision \
            \"m1caxjxlggy5xv63isfp5oxdbucx35efhgevxdklvlcgjgpdus3j3q\"\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().success()
        .stderr(ends_with("Database is up to date. \
            Last migration: \
            m1caxjxlggy5xv63isfp5oxdbucx35efhgevxdklvlcgjgpdus3j3q.\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("create-migration")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(4).stderr(ends_with("No schema changes detected.\n"));
    SERVER.admin_cmd()
        .arg("migration-log")
        .arg("--from-fs")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .arg("--newest-first")
        .assert().code(0)
        .stdout("\
            m1caxjxlggy5xv63isfp5oxdbucx35efhgevxdklvlcgjgpdus3j3q\n\
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa\n\
        ");
    SERVER.admin_cmd()
        .arg("migration-log")
        .arg("--from-fs")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(0)
        .stdout("\
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa\n\
            m1caxjxlggy5xv63isfp5oxdbucx35efhgevxdklvlcgjgpdus3j3q\n\
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
        .stderr(ends_with(
r###"error: Unexpected 'create'
  ┌─ tests/migrations/db1/error/bad.esdl:3:9
  │
3 │         create property text -> str;
  │         ^^^^^^^ error

edgedb error: cannot proceed until .esdl files are fixed
"###));
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
        .stderr(ends_with("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001.edgeql)\n"));

    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.arg("--database=modified2");
        cmd.arg("create-migration");
        cmd.arg("--schema-dir=tests/migrations/db1/modified2");
    });
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("y").unwrap();
    cmd.exp_string("Created \
        tests/migrations/db1/modified2/migrations/00002.edgeql, \
        id: m1caxjxlggy5xv63isfp5oxdbucx35efhgevxdklvlcgjgpdus3j3q").unwrap();

    SERVER.admin_cmd()
        .arg("--database=modified2")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified2")
        .assert().success()
        .stderr(ends_with("Applied \
            m1caxjxlggy5xv63isfp5oxdbucx35efhgevxdklvlcgjgpdus3j3q \
            (00002.edgeql)\n"));
    SERVER.admin_cmd()
        .arg("--database=modified2")
        .arg("show-status")
        .arg("--schema-dir=tests/migrations/db1/modified2")
        .assert().success()
        .stderr(ends_with("Database is up to date. \
            Last migration: \
            m1caxjxlggy5xv63isfp5oxdbucx35efhgevxdklvlcgjgpdus3j3q.\n"));
    SERVER.admin_cmd()
        .arg("--database=modified2")
        .arg("create-migration")
        .arg("--schema-dir=tests/migrations/db1/modified2")
        .assert().code(4).stderr(ends_with("No schema changes detected.\n"));
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
        .stderr(ends_with("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001.edgeql)\n"));

    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.arg("--database=modified3");
        cmd.arg("create-migration");
        cmd.arg("--schema-dir=tests/migrations/db1/modified3");
    });
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes").unwrap();
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes").unwrap();
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("back").unwrap();
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes").unwrap();
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes").unwrap();
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
        .assert().code(4).stderr(ends_with("No schema changes detected.\n"));
    Ok(())
}

#[test]
fn prompt_id() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db2/migrations/00001.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("create-database").arg("db2")
        .assert().success();
    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.arg("--database=db2");
        cmd.arg("create-migration");
        cmd.arg("--schema-dir=tests/migrations/db2");
    });
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes").unwrap();
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes").unwrap();
    // on pre-prompt_id version this would require an extra prompt
    cmd.exp_string("extra DDL statements").unwrap();
    cmd.exp_string("Created").unwrap();
    Ok(())
}

#[test]
fn input_required() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db3/migrations/00002.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("create-database").arg("db3")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=db3")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db3")
        .assert().success()
        .stderr(ends_with("Applied \
            m1d6kfhjnqmrw4lleqvx6fibf5hpmndpw2tn2f6o4wm6fjyf55dhcq \
            (00001.edgeql)\n"));

    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.arg("--database=db3");
        cmd.arg("create-migration");
        cmd.arg("--schema-dir=tests/migrations/db3");
    });
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes").unwrap();
    cmd.exp_string("cast_expr>").unwrap();
    cmd.send_line(".foo[IS Child2]").unwrap();
    cmd.exp_string("Created").unwrap();

    fs::remove_file("tests/migrations/db3/migrations/00002.edgeql").unwrap();
    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.arg("--database=db3");
        cmd.arg("create-migration");
        cmd.arg("--schema-dir=tests/migrations/db3");
    });
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes").unwrap();
    cmd.exp_string("cast_expr>").unwrap();
    cmd.send_line(".foo[IS Child2] # comment").unwrap();
    cmd.exp_string("Created").unwrap();
    Ok(())
}
