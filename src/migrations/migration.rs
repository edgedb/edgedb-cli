use std::collections::hash_map::{HashMap, Entry};
use std::ffi::OsStr;

use async_std::fs;
use async_std::path::{Path, PathBuf};
use async_std::stream::StreamExt;
use sha2::digest::Digest;
use edgeql_parser::tokenizer::TokenStream;
use fn_error_context::context;
use linked_hash_map::LinkedHashMap;

use crate::migrations::NULL_MIGRATION;
use crate::migrations::context::Context;
use crate::migrations::grammar::parse_migration;


#[derive(Debug)]
pub struct Migration {
    pub message: Option<String>,
    pub id: String,
    pub parent_id: String,
    pub text_range: (usize, usize),
}

#[derive(Debug)]
pub struct MigrationFile {
    path: PathBuf,
    data: Migration,
}

#[derive(PartialOrd, PartialEq, Eq, Ord)]
pub enum SortKey<'a> {
    Numeric(u64),
    Text(&'a OsStr),
}

fn validate_text(text: &str, migration: &Migration) -> anyhow::Result<()> {
    if migration.id.starts_with("m1") {
        let txt = &text[migration.text_range.0..migration.text_range.1];
        let mut hasher = sha2::Sha256::new();
        hasher.update(b"CREATE\0MIGRATION\0ONTO\0");
        hasher.update(migration.parent_id.as_bytes());
        hasher.update(b"\0{\0");
        for token in &mut TokenStream::new(txt) {
            let token = token.map_err(|e| anyhow::anyhow!("{}", e))?;
            hasher.update(token.token.value.as_bytes());
            hasher.update(b"\0");
        }
        hasher.update(b"\0}");
        let hash = base32::encode(
            base32::Alphabet::RFC4648 { padding: false },
            &hasher.finalize());
        let id = format!("m1{}", hash.to_ascii_lowercase());
        if id != migration.id {
            anyhow::bail!("migration name should be `{computed}` \
                but `{file}` is used instead.\n\
                Migration names are computed from the hash \
                of the migration contents. To proceed you must fix the \
                statement to read as:\n  \
                CREATE MIGRATION {computed} ONTO ...\n
                if this migration is not applied to \
                any database or revert the changes to the file",
                computed=id, file=migration.id);
        }
        Ok(())
    } else {
        anyhow::bail!("unknown version of migration id {:?}", migration.id);
    }
}

#[context("error reading migration file {}", path.display())]
async fn read_file(path: &Path, validate_hashes:bool)
    -> anyhow::Result<Migration>
{
    let text = fs::read_to_string(&path).await?;
    let data = parse_migration(&text)?;
    if validate_hashes {
        validate_text(&text, &data)?;
    }
    return Ok(data)
}

fn file_num(path: &Path) -> Option<u64> {
    path.file_stem().and_then(|x| x.to_str()).and_then(|x| x.parse().ok())
}

#[context("error reading migrations in {}", dir.display())]
async fn _read_all(dir: &Path, validate_hashes: bool)
    -> anyhow::Result<LinkedHashMap<String, MigrationFile>>
{
    let mut dir = fs::read_dir(dir).await?;
    let mut all = HashMap::new();
    while let Some(item) = dir.next().await.transpose()? {
        let fname = item.file_name();
        let lossy_name = fname.to_string_lossy();
        if lossy_name.starts_with(".") || !lossy_name.ends_with(".edgeql")
            || !item.file_type().await?.is_file()
        {
            continue;
        }
        let path = item.path();
        let data = read_file(&path, validate_hashes).await?;
        match all.entry(data.parent_id.clone()) {
            Entry::Vacant(v) => {
                v.insert(MigrationFile {
                    path: path.to_path_buf(),
                    data,
                });
            }
            Entry::Occupied(o) => {
                anyhow::bail!("Two files {:?} and {:?} have the same \
                    parent revision {:?}. Multiple branches in revision \
                    history are not supported yet, please rebase one of the \
                    branches on top of other.",
                    path, o.get().path, data.parent_id);
            }
        }
    }
    sort_revisions(all)
}

fn sort_revisions(mut all: HashMap<String, MigrationFile>)
    -> anyhow::Result<LinkedHashMap<String, MigrationFile>>
{
    let mut res = LinkedHashMap::new();
    let mut counter = 1;
    let mut parent_id = String::from(NULL_MIGRATION);
    while !all.is_empty() {
        if let Some(item) = all.remove(&parent_id) {
            if file_num(&item.path).map(|n| n != counter).unwrap_or(true) {
                anyhow::bail!("File `{}` should be named `{:05}.edgeql`",
                    item.path.display(), counter);
            }
            counter += 1;
            parent_id = item.data.id.clone();
            res.insert(item.data.id.clone(), item);
        } else {
            let next = all.values()
                .min_by_key(|item| {
                    match file_num(&item.path) {
                        Some(n) => SortKey::Numeric(n),
                        None => SortKey::Text(item.path.file_stem().unwrap()),
                    }
                })
                .unwrap();
            let valid_number = file_num(&next.path)
                .map(|n| n == counter)
                .unwrap_or(false);
            if valid_number {
                anyhow::bail!("File `{}` should have parent migration {:?} \
                    (`CREATE MIGRATION {} ONTO {} {{...`)",
                    next.path.display(), parent_id,
                    next.data.id, parent_id);
            } else {
                anyhow::bail!("Missing file `{:05}.edgeql` with \
                    parent migration {:?} (perhaps {} should be fixed?)",
                    counter, parent_id, next.path.display());
            }
        }
    }
    Ok(res)
}

pub async fn read_all(ctx: &Context, validate_hashes: bool)
    -> anyhow::Result<LinkedHashMap<String, MigrationFile>>
{
    _read_all(ctx.schema_dir.join("migrations").as_ref(), validate_hashes)
        .await
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use super::{Migration, MigrationFile};
    use super::{validate_text, parse_migration, sort_revisions};
    use crate::migrations::NULL_MIGRATION;

    #[test]
    #[should_panic(expected=
        "migration name should be \
        `m154kc2cbzmzz2tzcjz5rpsspdew3azydwhwpkhcgkznpp6ibwhevq` \
        but `m124` is used instead.")]
    fn test_bad_hash() {
        let text = r###"
            CREATE MIGRATION m124 ONTO initial {
            };
        "###;
        let migr = Migration {
            id: "m124".into(),
            parent_id: "initial".into(),
            message: None,
            text_range: (62, 62),
        };
        let result = parse_migration(text).unwrap();
        assert_eq!(result.id, migr.id);
        assert_eq!(result.parent_id, migr.parent_id);
        assert_eq!(result.message, migr.message);
        assert_eq!(result.text_range, migr.text_range);
        validate_text(text, &migr).unwrap();
    }

    #[test]
    fn test_hash_zero() {
        let text = r###"
            CREATE MIGRATION
                m154kc2cbzmzz2tzcjz5rpsspdew3azydwhwpkhcgkznpp6ibwhevq
                ONTO initial
            {
            };
        "###;
        let migr = Migration {
            id: "m154kc2cbzmzz2tzcjz5rpsspdew3azydwhwpkhcgkznpp6ibwhevq"
                .into(),
            parent_id: "initial".into(),
            message: None,
            text_range: (156, 156),
        };
        let result = parse_migration(text).unwrap();
        assert_eq!(result.id, migr.id);
        assert_eq!(result.parent_id, migr.parent_id);
        assert_eq!(result.message, migr.message);
        assert_eq!(result.text_range, migr.text_range);
        validate_text(text, &migr).unwrap();
    }

    #[test]
    fn test_hash_1() {
        let text = r###"
            CREATE MIGRATION
                m1zqdy6fkelif6cnwwwkmyvk5gnsbfkhrmnbitopt6plk3kp2fqpha
                ONTO m1g3qzqdr57pp3w2mdwdkq4g7dq4oefawqdavzgeiov7fiwntpb3lq
            {
                CREATE TYPE Type1;
            };
        "###;
        let migr = Migration {
            id: "m1zqdy6fkelif6cnwwwkmyvk5gnsbfkhrmnbitopt6plk3kp2fqpha"
                .into(),
            parent_id: "m1g3qzqdr57pp3w2mdwdkq4g7dq4oefawqdavzgeiov7fiwntpb3lq"
                .into(),
            message: None,
            text_range: (207, 238),
        };
        let result = parse_migration(text).unwrap();
        assert_eq!(result.id, migr.id);
        assert_eq!(result.parent_id, migr.parent_id);
        assert_eq!(result.message, migr.message);
        assert_eq!(result.text_range, migr.text_range);
        validate_text(text, &migr).unwrap();
    }

    #[test]
    #[should_panic(expected=
        "migration name should be \
        `m1l2x6ndfuxijzutz4yil6owejrqoptramv2kmcfqu6wihxi5p3qsa` \
        but `m1nsp3k6jku6qckffo33as5pntqgy62z45w73afoys6qjjkk62r2lq` \
        is used instead")]
    fn test_hash_depends_on_parent() {
        let text = r###"
            CREATE MIGRATION
                m1nsp3k6jku6qckffo33as5pntqgy62z45w73afoys6qjjkk62r2lq
                ONTO initial
            {
                CREATE TYPE Type1;
            };
        "###;
        let migr = Migration {
            id: "m1nsp3k6jku6qckffo33as5pntqgy62z45w73afoys6qjjkk62r2lq"
                .into(),
            parent_id: "initial".into(),
            message: None,
            text_range: (160, 191),
        };
        let result = parse_migration(text).unwrap();
        assert_eq!(result.id, migr.id);
        assert_eq!(result.parent_id, migr.parent_id);
        assert_eq!(result.message, migr.message);
        assert_eq!(result.text_range, migr.text_range);
        validate_text(text, &migr).unwrap();
    }

    #[test]
    fn sort_empty() {
        assert!(sort_revisions(HashMap::new()).unwrap().is_empty());
    }

    fn mk_seq(input: &[(&str, &str, &str)]) -> HashMap<String, MigrationFile> {
        input.iter().cloned().map(|(id, parent, filename)| {
            (parent.into(), MigrationFile {
                path: filename.into(),
                data: Migration {
                    id: id.into(),
                    parent_id: parent.into(),
                    message: None,
                    text_range: (0, 0),
                }
            })
        }).collect()
    }

    #[test]
    fn sort_single() {
        assert_eq!(sort_revisions(mk_seq(&[
            ("m10001", NULL_MIGRATION, "0001.edgeql"),
        ])).unwrap().keys().collect::<Vec<_>>(), &["m10001"]);
    }

    #[test]
    #[should_panic(expected="File `0001.edgeql` should have parent migration \
        \"initial\"")]
    fn first_wrong_parent() {
        sort_revisions(mk_seq(&[
            ("m10001", "m10002", "0001.edgeql"),
        ])).unwrap();
    }

    #[test]
    #[should_panic(expected="File `0002.edgeql` should be named `00001.edgeql`")]
    fn first_wrong_filename() {
        sort_revisions(mk_seq(&[
            ("m10001", NULL_MIGRATION, "0002.edgeql"),
        ])).unwrap();
    }

    #[test]
    fn sort_two() {
        assert_eq!(sort_revisions(mk_seq(&[
            ("m10001", NULL_MIGRATION, "0001.edgeql"),
            ("m10002", "m10001", "0002.edgeql"),
        ])).unwrap().keys().collect::<Vec<_>>(), &["m10001", "m10002"]);
    }

    #[test]
    #[should_panic(expected="File `some.edgeql` should be \
                             named `00001.edgeql`")]
    fn two_filename_bad1() {
        sort_revisions(mk_seq(&[
            ("m10001", NULL_MIGRATION, "some.edgeql"),
            ("m10002", "m10001", "0002.edgeql"),
        ])).unwrap();
    }

    #[test]
    #[should_panic(expected="File `0003.edgeql` should be \
                             named `00001.edgeql`")]
    fn two_filename_non_seq1() {
        sort_revisions(mk_seq(&[
            ("m10001", NULL_MIGRATION, "0003.edgeql"),
            ("m10002", "m10001", "0002.edgeql"),
        ])).unwrap();
    }

    #[test]
    #[should_panic(expected="File `some.edgeql` should be \
                             named `00002.edgeql`")]
    fn two_filename_bad2() {
        sort_revisions(mk_seq(&[
            ("m10001", NULL_MIGRATION, "0001.edgeql"),
            ("m10002", "m10001", "some.edgeql"),
        ])).unwrap();
    }

    #[test]
    #[should_panic(expected="File `0003.edgeql` should be \
                             named `00002.edgeql`")]
    fn two_filename_non_seq2() {
        sort_revisions(mk_seq(&[
            ("m10001", NULL_MIGRATION, "0001.edgeql"),
            ("m10002", "m10001", "0003.edgeql"),
        ])).unwrap();
    }

    #[test]
    #[should_panic(expected="Missing file `00002.edgeql` with parent \
        migration \"m10001\" (perhaps 0003.edgeql should be fixed?)")]
    fn two_missing_second() {
        sort_revisions(mk_seq(&[
            ("m10001", NULL_MIGRATION, "0001.edgeql"),
            ("m10003", "m10002", "0003.edgeql"),
            ("m10004", "m10003", "0004.edgeql"),
        ])).unwrap();
    }

    #[test]
    #[should_panic(expected="File `0002.edgeql` should have \
        parent migration \"m10001\"")]
    fn two_bad_next_parent() {
        sort_revisions(mk_seq(&[
            ("m10001", NULL_MIGRATION, "0001.edgeql"),
            ("m10003", "m10002", "0002.edgeql"),
            ("m10004", "m10003", "0003.edgeql"),
        ])).unwrap();
    }

}
