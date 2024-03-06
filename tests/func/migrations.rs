use std::fs;
use std::path::Path;
use crate::SERVER;
use predicates::str::{ends_with, contains};


#[test]
fn bare_status() -> anyhow::Result<()> {
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("empty")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=empty")
        .arg("migration").arg("status")
        .arg("--schema-dir=tests/migrations/db1/bare")
        .assert().code(2)
        .stderr(contains("CREATE PROPERTY field1"))
        .stderr(contains("edgedb error: Some migrations are missing"));
    Ok(())
}

#[test]
fn initial() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db1/initial/migrations/00002-m1e5vq3.edgeql")
        .ok();
    fs::remove_file("tests/migrations/db1/initial/migrations/00003-m1wrvvw.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("initial")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migration").arg("status")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(3)
        .stderr(ends_with(
            "edgedb error: Database is empty, while 1 migrations \
            have been found in the filesystem.\n  Run `edgedb migrate` to apply.\n"));
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("query").arg("SELECT cfg::DatabaseConfig.allow_bare_ddl")
        .assert().success()
        .stdout("\"AlwaysAllow\"\n");
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migration").arg("create")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(1).stderr(ends_with(
            "edgedb error: Database must be updated \
            to the last migration on the filesystem for `migration create`. \
            Run:\n  \
              edgedb migrate\n"));
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().success()
        .stderr(contains("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001-m12bulr.edgeql)\n"));
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migration").arg("log")
        .arg("--from-db")
        .arg("--newest-first")
        .arg("--limit=1")
        .assert().code(0)
        .stdout("m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa\n");
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("query").arg("SELECT cfg::DatabaseConfig.allow_bare_ddl")
        .assert().success()
        .stdout("\"NeverAllow\"\n");
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migration").arg("status")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().success()
        .stderr(ends_with("Database is up to date. \
            Last migration: \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa.\n"));
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migration").arg("create")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(4).stderr(ends_with("No schema changes detected.\n"));
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migration").arg("create")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(4).stderr(ends_with("No schema changes detected.\n"));

    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migration").arg("create")
        .arg("--allow-empty")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().code(0)
        .stderr(ends_with("Created \
            tests/migrations/db1/initial/migrations/00002-m1e5vq3.edgeql, \
            id: m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq\n"));
    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .assert().success()
        .stderr(ends_with("Applied \
            m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq \
            (00002-m1e5vq3.edgeql)\n"));

    SERVER.admin_cmd()
        .arg("--database=initial")
        .arg("migration").arg("create")
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
            (00003-m1wrvvw.edgeql)\n"));

    // Now test partial migrations
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("initial_2")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=initial_2")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .arg("--to-revision=m1e5vq3h4oizlsp4a3zge5bqh")
        .assert().success()
        .stderr(contains("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001-m12bulr.edgeql)\n\
            Applied \
            m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq \
            (00002-m1e5vq3.edgeql)\n"));

    SERVER.admin_cmd()
        .arg("--database=initial_2")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .arg("--to-revision=m12bulrbo")
        .assert().success()
        .stderr(ends_with("Database is up to date. \
            Revision m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            is the ancestor of the latest \
            m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq\n"));

    SERVER.admin_cmd()
        .arg("--database=initial_2")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .arg("--to-revision=m1e5vq3h4oizlsp4a")
        .assert().success()
        .stderr(ends_with("Database is up to date. Revision \
            m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq\n"));

    SERVER.admin_cmd()
        .arg("--database=initial_2")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/initial")
        .arg("--to-revision=m1wrvvw3lycy")
        .assert().success()
        .stderr(ends_with("Applied \
            m1wrvvw3lycyovtlx4szqm75554g75h5nnbjq3a5qsdncn3oef6nia \
            (00003-m1wrvvw.edgeql)\n"));

    Ok(())
}

#[test]
fn project() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db1/project/priv/dbschema/migrations/00002-m1e5vq3.edgeql")
        .ok();
    fs::remove_file("tests/migrations/db1/project/priv/dbschema/migrations/00003-m1wrvvw.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("project")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=project")
        .arg("migration").arg("status")
        .current_dir("tests/migrations/db1/project")
        .assert().code(3)
        .stderr(ends_with(
            "edgedb error: Database is empty, while 1 migrations \
            have been found in the filesystem.\n  Run `edgedb migrate` to apply.\n"));
    SERVER.admin_cmd()
        .arg("--database=project")
        .arg("query").arg("SELECT cfg::DatabaseConfig.allow_bare_ddl")
        .assert().success()
        .stdout("\"AlwaysAllow\"\n");
    SERVER.admin_cmd()
        .arg("--database=project")
        .arg("migration").arg("create")
        .arg("--non-interactive")
        .current_dir("tests/migrations/db1/project")
        .assert().code(1).stderr(ends_with(
            "edgedb error: Database must be updated \
            to the last migration on the filesystem for `migration create`. \
            Run:\n  \
              edgedb migrate\n"));
    SERVER.admin_cmd()
        .arg("--database=project")
        .arg("migrate")
        .current_dir("tests/migrations/db1/project")
        .assert().success()
        .stderr(contains("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001-m12bulr.edgeql)\n"));
    SERVER.admin_cmd()
        .arg("--database=project")
        .arg("query").arg("SELECT cfg::DatabaseConfig.allow_bare_ddl")
        .assert().success()
        .stdout("\"NeverAllow\"\n");
    SERVER.admin_cmd()
        .arg("--database=project")
        .arg("migration").arg("status")
        .current_dir("tests/migrations/db1/project")
        .assert().success()
        .stderr(ends_with("Database is up to date. \
            Last migration: \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa.\n"));
    SERVER.admin_cmd()
        .arg("--database=project")
        .arg("migration").arg("create")
        .arg("--non-interactive")
        .current_dir("tests/migrations/db1/project")
        .assert().code(4).stderr(ends_with("No schema changes detected.\n"));
    SERVER.admin_cmd()
        .arg("--database=project")
        .arg("migration").arg("create")
        .current_dir("tests/migrations/db1/project")
        .assert().code(4).stderr(ends_with("No schema changes detected.\n"));

    SERVER.admin_cmd()
        .arg("--database=project")
        .arg("migration").arg("create")
        .arg("--allow-empty")
        .current_dir("tests/migrations/db1/project")
        .assert().code(0)
        .stderr(ends_with("Created \
            ./priv/dbschema/migrations/00002-m1e5vq3.edgeql, \
            id: m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq\n"));
    SERVER.admin_cmd()
        .arg("--database=project")
        .arg("migrate")
        .current_dir("tests/migrations/db1/project")
        .assert().success()
        .stderr(ends_with("Applied \
            m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq \
            (00002-m1e5vq3.edgeql)\n"));

    SERVER.admin_cmd()
        .arg("--database=project")
        .arg("migration").arg("create")
        .arg("--allow-empty")
        .arg("--non-interactive")
        .current_dir("tests/migrations/db1/project")
        .assert().code(0);
    SERVER.admin_cmd()
        .arg("--database=project")
        .arg("migrate")
        .current_dir("tests/migrations/db1/project")
        .assert().success()
        .stderr(ends_with("Applied \
            m1wrvvw3lycyovtlx4szqm75554g75h5nnbjq3a5qsdncn3oef6nia \
            (00003-m1wrvvw.edgeql)\n"));

    // Now test partial migrations
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("project_2")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=project_2")
        .arg("migrate")
        .current_dir("tests/migrations/db1/project")
        .arg("--to-revision=m1e5vq3h4oizlsp4a3zge5bqh")
        .assert().success()
        .stderr(contains("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001-m12bulr.edgeql)\n\
            Applied \
            m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq \
            (00002-m1e5vq3.edgeql)\n"));

    SERVER.admin_cmd()
        .arg("--database=project_2")
        .arg("migrate")
        .current_dir("tests/migrations/db1/project")
        .arg("--to-revision=m12bulrbo")
        .assert().success()
        .stderr(ends_with("Database is up to date. \
            Revision m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            is the ancestor of the latest \
            m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq\n"));

    SERVER.admin_cmd()
        .arg("--database=project_2")
        .arg("migrate")
        .current_dir("tests/migrations/db1/project")
        .arg("--to-revision=m1e5vq3h4oizlsp4a")
        .assert().success()
        .stderr(ends_with("Database is up to date. Revision \
            m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq\n"));

    SERVER.admin_cmd()
        .arg("--database=project_2")
        .arg("migrate")
        .current_dir("tests/migrations/db1/project")
        .arg("--to-revision=m1wrvvw3lycy")
        .assert().success()
        .stderr(ends_with("Applied \
            m1wrvvw3lycyovtlx4szqm75554g75h5nnbjq3a5qsdncn3oef6nia \
            (00003-m1wrvvw.edgeql)\n"));

    Ok(())
}

#[test]
fn modified1() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db1/modified1/migrations/00002-m13wjyi.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("modified1")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migration").arg("status")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(3)
        .stderr(ends_with(
            "edgedb error: Database is empty, while 1 migrations \
            have been found in the filesystem.\n  Run `edgedb migrate` to apply.\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migration").arg("create")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(1).stderr(ends_with(
            "edgedb error: Database must be updated \
            to the last migration on the filesystem for `migration create`. \
            Run:\n  \
              edgedb migrate\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().success()
        .stderr(contains("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001-m12bulr.edgeql)\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migration").arg("status")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(2)
        .stderr(contains("CREATE PROPERTY field2"))
        .stderr(contains("edgedb error: Some migrations are missing"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migration").arg("create")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(0);
    SERVER.admin_cmd()
        .arg("migration").arg("log")
        .arg("--from-fs")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .arg("--newest-first")
        .arg("--limit=1")
        .assert().code(0)
        .stdout("m13wjyiog2dbum2ou32yp77eysbewews7vlv6rqqfswpyi2yd4s55a\n");
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migration").arg("status")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(3)
        .stderr(ends_with("Database is at migration \
            \"m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa\" \
            while sources contain 1 migrations ahead, \
            starting from \
            \"m13wjyiog2dbum2ou32yp77eysbewews7vlv6rqqfswpyi2yd4s55a\"\
            (tests/migrations/db1/modified1/migrations/00002-m13wjyi.edgeql)\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().success()
        .stderr(ends_with("Applied \
            m13wjyiog2dbum2ou32yp77eysbewews7vlv6rqqfswpyi2yd4s55a \
            (00002-m13wjyi.edgeql)\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(0)
        .stderr(ends_with("Everything is up to date. Revision \
            m13wjyiog2dbum2ou32yp77eysbewews7vlv6rqqfswpyi2yd4s55a\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migration").arg("status")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().success()
        .stderr(ends_with("Database is up to date. \
            Last migration: \
            m13wjyiog2dbum2ou32yp77eysbewews7vlv6rqqfswpyi2yd4s55a.\n"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migration").arg("create")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(4).stderr(ends_with("No schema changes detected.\n"));
    SERVER.admin_cmd()
        .arg("migration").arg("log")
        .arg("--from-fs")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .arg("--newest-first")
        .assert().code(0)
        .stdout("\
            m13wjyiog2dbum2ou32yp77eysbewews7vlv6rqqfswpyi2yd4s55a\n\
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa\n\
        ");
    SERVER.admin_cmd()
        .arg("migration").arg("log")
        .arg("--from-fs")
        .arg("--schema-dir=tests/migrations/db1/modified1")
        .assert().code(0)
        .stdout("\
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa\n\
            m13wjyiog2dbum2ou32yp77eysbewews7vlv6rqqfswpyi2yd4s55a\n\
        ");

    fs::remove_dir_all("tests/migrations/db1/squash").ok();
    fs::create_dir_all("tests/migrations/db1/squash")?;
    fs::create_dir_all("tests/migrations/db1/squash/migrations")?;
    fs::copy("tests/migrations/db1/modified1/default.esdl",
             "tests/migrations/db1/squash/default.esdl")?;
    fs::copy("tests/migrations/db1/modified1/migrations/00001-m12bulr.edgeql",
             "tests/migrations/db1/squash/migrations/00001-m12bulr.edgeql")?;
    fs::copy("tests/migrations/db1/modified1/migrations/00002-m13wjyi.edgeql",
             "tests/migrations/db1/squash/migrations/00002-m13wjyi.edgeql")?;

    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migration").arg("create")
        .arg("--squash")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/squash")
        .assert().success().stderr(contains("Squash is complete"));
    SERVER.admin_cmd()
        .arg("--database=modified1")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/squash")
        .assert().success().stderr(ends_with(
            "m1fw3q62du3fmdbeuikq3tc4fsfhs3phafnjhoh3jzedk3sfgx3lha\
            .edgeql)\n"
        ));
    SERVER.admin_cmd()
        .arg("migration").arg("log")
        .arg("--from-fs")
        .arg("--schema-dir=tests/migrations/db1/squash")
        .assert().code(0)
        .stdout("\
            m1fw3q62du3fmdbeuikq3tc4fsfhs3phafnjhoh3jzedk3sfgx3lha\n\
        ");
    SERVER.admin_cmd()
        .arg("migration").arg("log")
        .arg("--database=modified1")
        .arg("--from-db")
        .arg("--schema-dir=tests/migrations/db1/squash")
        .assert().code(0)
        .stdout("\
            m1fw3q62du3fmdbeuikq3tc4fsfhs3phafnjhoh3jzedk3sfgx3lha\n\
        ");
    Ok(())
}

#[test]
fn error() -> anyhow::Result<()> {
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("empty_err")
        .assert().success();
    let err = if SERVER.version().major >= 4 {
r###"error: Unexpected keyword 'CREATE'
  ┌─ tests/migrations/db1/error/bad.esdl:3:9
  │
3 │         create property text -> str;
  │         ^^^^^^ Use a different identifier or quote the name with backticks: `create`
  │
  = This name is a reserved keyword and cannot be used as an identifier

edgedb error: cannot proceed until .esdl files are fixed
"###
    } else {
r###"error: Unexpected keyword 'CREATE'
  ┌─ tests/migrations/db1/error/bad.esdl:3:9
  │
3 │         create property text -> str;
  │         ^^^^^^ error

edgedb error: cannot proceed until .esdl files are fixed
"###
    };
    SERVER.admin_cmd()
        .arg("--database=empty_err")
        .arg("migration").arg("status")
        .arg("--schema-dir=tests/migrations/db1/error")
        .env("NO_COLOR", "1")
        .assert().code(1)
        .stderr(ends_with(err));
    Ok(())
}

#[test]
fn modified2_interactive() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db1/modified2/migrations/00002-m13wjyi.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("modified2")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=modified2")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified2")
        .assert().success()
        .stderr(contains("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001-m12bulr.edgeql)\n"));

    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.arg("--database=modified2");
        cmd.arg("migration").arg("create");
        cmd.arg("--schema-dir=tests/migrations/db1/modified2");
    });
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("y").unwrap();
    cmd.exp_string("Created \
        tests/migrations/db1/modified2/migrations/00002-m13wjyi.edgeql, \
        id: m13wjyiog2dbum2ou32yp77eysbewews7vlv6rqqfswpyi2yd4s55a").unwrap();

    SERVER.admin_cmd()
        .arg("--database=modified2")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified2")
        .assert().success()
        .stderr(ends_with("Applied \
            m13wjyiog2dbum2ou32yp77eysbewews7vlv6rqqfswpyi2yd4s55a \
            (00002-m13wjyi.edgeql)\n"));
    SERVER.admin_cmd()
        .arg("--database=modified2")
        .arg("migration").arg("status")
        .arg("--schema-dir=tests/migrations/db1/modified2")
        .assert().success()
        .stderr(ends_with("Database is up to date. \
            Last migration: \
            m13wjyiog2dbum2ou32yp77eysbewews7vlv6rqqfswpyi2yd4s55a.\n"));
    SERVER.admin_cmd()
        .arg("--database=modified2")
        .arg("migration").arg("create")
        .arg("--schema-dir=tests/migrations/db1/modified2")
        .assert().code(4).stderr(ends_with("No schema changes detected.\n"));
    Ok(())
}

#[test]
fn modified3_interactive() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db1/modified3/migrations/00002.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("modified3")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=modified3")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db1/modified3")
        .assert().success()
        .stderr(contains("Applied \
            m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa \
            (00001.edgeql)\n"));

    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.arg("--database=modified3");
        cmd.arg("migration").arg("create");
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
        .arg("migration").arg("status")
        .arg("--schema-dir=tests/migrations/db1/modified3")
        .assert().success();  // revision can be different because of order
    SERVER.admin_cmd()
        .arg("--database=modified3")
        .arg("migration").arg("create")
        .arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db1/modified3")
        .assert().code(4).stderr(ends_with("No schema changes detected.\n"));
    Ok(())
}

#[test]
fn prompt_id() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db2/initial/migrations/00001-m1fvz72.edgeql")
        .ok();
    fs::remove_file("tests/migrations/db2/modified1/migrations/00002-m1u6tot.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("db2")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=db2")
        .arg("migration").arg("create")
        .arg("--schema-dir=tests/migrations/db2/initial")
        .assert().success();  // should not ask any questions on first rev
    SERVER.admin_cmd()
        .arg("--database=db2")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db2/initial")
        .assert().success()
        .stderr(contains("Applied \
            m1fvz72asuad3xkor4unxshp524wp6stgdnbd34vxvjfjkrzemonkq \
            (00001-m1fvz72.edgeql)\n"));

    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.arg("--database=db2");
        cmd.arg("migration").arg("create");
        cmd.arg("--schema-dir=tests/migrations/db2/modified1");
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
    fs::remove_file("tests/migrations/db3/migrations/00002-m1bdkut.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("db3")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=db3")
        .arg("migrate")
        .arg("--schema-dir=tests/migrations/db3")
        .assert().success()
        .stderr(contains("Applied \
            m1d6kfhjnqmrw4lleqvx6fibf5hpmndpw2tn2f6o4wm6fjyf55dhcq \
            (00001-m1d6kfh.edgeql)\n"));

    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.arg("--database=db3");
        cmd.arg("migration").arg("create");
        cmd.arg("--schema-dir=tests/migrations/db3");
    });
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes").unwrap();
    cmd.exp_string("cast_expr>").unwrap();
    cmd.send_line("").unwrap();  // default value
    cmd.exp_string("Created").unwrap();

    fs::remove_file("tests/migrations/db3/migrations/00002-m1bdkut.edgeql").unwrap();
    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.arg("--database=db3");
        cmd.arg("migration").arg("create");
        cmd.arg("--schema-dir=tests/migrations/db3");
    });
    cmd.exp_string("[y,n,l,c,b,s,q,?]").unwrap();
    cmd.send_line("yes").unwrap();
    cmd.exp_string("cast_expr>").unwrap();
    // just add a comment to the default value
    cmd.send_line("# comment").unwrap();
    cmd.exp_string("Created").unwrap();
    Ok(())
}

#[test]
fn eof_err() -> anyhow::Result<()> {
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("db_eof_err")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=db_eof_err")
        .arg("migration").arg("create")
        .arg("--schema-dir=tests/migrations/db_eof_err")
        .env("NO_COLOR", "1")
        .assert().code(1)
        .stderr(ends_with(r###"error: Missing '{'
   ┌─ tests/migrations/db_eof_err/default.esdl:9:19
   │  
 9 │   alias default::Foo
   │ ╭──────────────────^
10 │ │ 
   │ ╰^ error

edgedb error: cannot proceed until .esdl files are fixed
"###));
    Ok(())
}

#[test]
fn dev_mode() -> anyhow::Result<()> {
    fs::remove_file("tests/migrations/db4/modified1/migrations/00001-m1qfgvb.edgeql")
        .ok();
    fs::remove_file("tests/migrations/db4/created1/migrations/00002-m1d3iwv.edgeql")
        .ok();
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("db4")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=db4")
        .arg("migrate").arg("--dev-mode")
        .arg("--schema-dir=tests/migrations/db4/initial")
        .env("NO_COLOR", "1")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=db4")
        .arg("migrate").arg("--dev-mode")
        .arg("--schema-dir=tests/migrations/db4/modified1")
        .env("NO_COLOR", "1")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=db4")
        .arg("migration").arg("create").arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db4/modified1")
        .env("NO_COLOR", "1")
        .assert().success();
    assert!(
        Path::new("tests/migrations/db4/modified1/migrations/00001-m1qfgvb.edgeql")
        .exists()
    );
    SERVER.admin_cmd()
        .arg("--database=db4")
        .arg("migrate").arg("--dev-mode")
        .arg("--schema-dir=tests/migrations/db4/created1")
        .env("NO_COLOR", "1")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=db4")
        .arg("migration").arg("create").arg("--non-interactive")
        .arg("--schema-dir=tests/migrations/db4/created1")
        .env("NO_COLOR", "1")
        .assert().success();
    assert!(
        Path::new("tests/migrations/db4/created1/migrations/00002-m1d3iwv.edgeql")
        .exists()
    );
    Ok(())
}

#[test]
fn unsafe_migrations() -> anyhow::Result<()> {
    SERVER.admin_cmd()
        .arg("database").arg("create").arg("db_unsafe")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=db_unsafe")
        .arg("migrate").arg("--dev-mode")
        .arg("--schema-dir=tests/migrations/db_unsafe/initial")
        .env("NO_COLOR", "1")
        .assert().success();
    SERVER.admin_cmd()
        .arg("--database=db_unsafe")
        .arg("migrate").arg("--dev-mode")
        .arg("--schema-dir=tests/migrations/db_unsafe/modified1")
        .env("NO_COLOR", "1")
        .assert().success();
    Ok(())
}
