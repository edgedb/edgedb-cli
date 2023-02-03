use std::collections::BTreeSet;

use crate::SERVER;


#[test]
fn configure_all_parameters() {
    let cmd = SERVER.admin_cmd()
        .arg("--tab-separated")
        .arg("query")
        .arg(r###"
            WITH Ptr := (SELECT schema::ObjectType
                         FILTER .name = 'cfg::Config'),
                 Props := (
                    SELECT Ptr.properties {
                        name,
                        is_internal := exists((SELECT .annotations
                            FILTER .name = "cfg::internal" AND @value = 'true'
                        )),
                   }
                ),
            SELECT Props { name }
            FILTER .name != 'id' AND NOT .is_internal
                // `force_database_error` should not be exposed
                AND .name != 'force_database_error'
        "###)
        .assert().success();
    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();
    let db_simple_options = out.lines().collect::<BTreeSet<_>>();

    let cmd = SERVER.admin_cmd()
        .arg("configure").arg("set")
        .arg("-h")
        .assert().success();
    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();
    let cmd_simple_options = out.lines()
        .skip_while(|line| line != &"SUBCOMMANDS:")
        .skip(1)
        .filter(|line| line.len() > 4)
        .filter(|line| !line[4..].starts_with("    "))
        .map(|line| line.split_whitespace().next().unwrap())
        .filter(|line| !line.is_empty() && line != &"help")
        .collect::<BTreeSet<_>>();

    if !db_simple_options.is_subset(&cmd_simple_options) {
        assert_eq!(db_simple_options, cmd_simple_options); // nice diff
    }

    let cmd = SERVER.admin_cmd()
        .arg("query")
        .arg("--output-format=tab-separated")
        .arg(r###"
            WITH Ptr := (SELECT schema::ObjectType
                         FILTER .name = 'cfg::Config'),
                 Links := (
                    SELECT Ptr.links {
                        name,
                        is_system := exists((SELECT .annotations
                            FILTER .name = "cfg::system" and @value = 'true'))
                    }
                ),
            SELECT Links { target_name := .target.name[5:] }
            FILTER .is_system AND .target.name[:5] = 'cfg::'
                // `force_database_error` should not be exposed
                AND .name != 'force_database_error'
        "###)
        .assert().success();
    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();
    let db_object_options = out.lines().collect::<BTreeSet<_>>();

    let cmd = SERVER.admin_cmd()
        .arg("configure").arg("insert")
        .arg("-h")
        .assert().success();
    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();
    let cmd_object_options = out.lines()
        .skip_while(|line| line != &"SUBCOMMANDS:")
        .skip(1)
        .filter(|line| line.len() > 4)
        .filter(|line| !line[4..].starts_with("    "))
        .map(|line| line.split_whitespace().next().unwrap())
        .filter(|line| !line.is_empty() && line != &"help")
        .collect::<BTreeSet<_>>();
    if !db_object_options.is_subset(&cmd_object_options) {
        assert_eq!(db_object_options, cmd_object_options); // nice diff
    }

    let cmd = SERVER.admin_cmd()
        .arg("configure").arg("reset")
        .arg("-h")
        .assert().success();
    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();
    let cmd_reset_options = out.lines()
        .skip_while(|line| line != &"SUBCOMMANDS:")
        .skip(1)
        .filter(|line| line.len() > 4)
        .filter(|line| !line[4..].starts_with("    "))
        .map(|line| line.split_whitespace().next().unwrap())
        .filter(|line| !line.is_empty() && line != &"help")
        .collect::<BTreeSet<_>>();
    let db_reset_options = db_object_options.union(&db_simple_options)
        .map(|x| *x).collect::<BTreeSet<_>>();
    if !db_reset_options.is_subset(&cmd_reset_options) {
        assert_eq!(db_reset_options, cmd_reset_options); // nice diff
    }
}
