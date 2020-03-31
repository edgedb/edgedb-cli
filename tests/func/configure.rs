use std::cmp::min;
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
        "###)
        .assert().success();
    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();
    let db_simple_options = out.lines().collect::<BTreeSet<_>>();

    let cmd = SERVER.admin_cmd()
        .arg("configure").arg("set")
        .arg("--help")
        .assert().success();
    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();
    let cmd_simple_options = out.lines()
        .skip_while(|line| line != &"SUBCOMMANDS:")
        .skip(1)
        .filter(|line| line.len() > 4)
        .map(|line| line[..min(33, line.len())].trim())
        .filter(|line| !line.is_empty() && line != &"help")
        .collect::<BTreeSet<_>>();

    assert_eq!(db_simple_options, cmd_simple_options);

    let cmd = SERVER.admin_cmd()
        .arg("--tab-separated")
        .arg("query")
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
        "###)
        .assert().success();
    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();
    let db_object_options = out.lines().collect::<BTreeSet<_>>();

    let cmd = SERVER.admin_cmd()
        .arg("configure").arg("insert")
        .arg("--help")
        .assert().success();
    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();
    let cmd_object_options = out.lines()
        .skip_while(|line| line != &"SUBCOMMANDS:")
        .skip(1)
        .filter(|line| line.len() > 4)
        .map(|line| line[..min(12, line.len())].trim())
        .filter(|line| !line.is_empty() && line != &"help")
        .collect::<BTreeSet<_>>();
    assert_eq!(db_object_options, cmd_object_options);

    let cmd = SERVER.admin_cmd()
        .arg("configure").arg("reset")
        .arg("--help")
        .assert().success();
    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();
    let cmd_reset_options = out.lines()
        .skip_while(|line| line != &"SUBCOMMANDS:")
        .skip(1)
        .filter(|line| line.len() > 4)
        .map(|line| line[..min(33, line.len())].trim())
        .filter(|line| !line.is_empty() && line != &"help")
        .collect::<BTreeSet<_>>();
    assert_eq!(
        db_object_options.union(&db_simple_options)
        .map(|x| *x).collect::<BTreeSet<_>>(),
        cmd_reset_options);
}
