#![cfg_attr(not(feature="portable_tests"), allow(dead_code, unused_imports))]

use std::fmt::{Display, Formatter};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use assert_cmd::Command;
use edgedb_protocol::model::Duration;
use predicates::Predicate;
use predicates::reflection::PredicateReflection;
use serde_json::Value;

struct ResultPredicate {
    result: Value,
}

impl Predicate<str> for ResultPredicate {
    fn eval(&self, variable: &str) -> bool {
        let actual: Value = serde_json::from_str(variable).unwrap();
        for (k, v) in actual.as_object().unwrap() {
            match self.result.get(k) {
                Some(expected) if k != "waitUntilAvailable" => {
                    if !expected.eq(v) {
                        println!("{}: {} != {}", k, v, expected);
                        return false;
                    }
                }
                Some(expected) => {
                    let expected = expected.as_str().unwrap().parse::<Duration>().unwrap();
                    if v.as_i64().is_none() {
                        panic!("illegal waitUntilAvailable: {}", v);
                    }
                    let v = Duration::from_micros(v.as_i64().unwrap());
                    if expected != v {
                        println!("{}: {} != {}", k, v, expected);
                        return false;
                    }
                }
                None => {
                    println!("{}={} was not expected", k, v);
                    return false;
                }
            }
        }
        for (k, v) in self.result.as_object().unwrap() {
            if actual.get(k).is_none() {
                println!("expect {}={}", k, v);
                return false;
            }
        }
        true
    }
}


impl PredicateReflection for ResultPredicate {}

impl Display for ResultPredicate {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.result.fmt(f)
    }
}

struct MockFile {
    path: PathBuf
}

impl Drop for MockFile {
    fn drop(&mut self) {
        fs::remove_file(&self.path).unwrap();
    }
}

fn mock_file(path: &str, content: &str) -> MockFile {
    let path = PathBuf::from(path);
    let parent = path.parent().unwrap();
    if !parent.exists() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap();
    MockFile { path }
}

fn expect(result: Value) -> ResultPredicate {
    ResultPredicate {
        result,
    }
}

include!(concat!(env!("OUT_DIR"), "/shared_client_testcases.rs"));
