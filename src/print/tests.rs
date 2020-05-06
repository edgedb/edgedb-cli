use std::convert::TryFrom;
use std::str::FromStr;

use bigdecimal::BigDecimal;

use edgedb_protocol::value::Value;
use edgedb_protocol::codec::{ObjectShape, ShapeElement};
use crate::print::{self, test_format, test_format_cfg, Config};


fn json_fmt(j: &str) -> String {
    print::json_to_string(
        serde_json::from_str::<serde_json::Value>(j).unwrap()
        .as_array().unwrap(),
        &Config::new())
    .unwrap()
}

fn json_fmt_width(w: usize, j: &str) -> String {
    print::json_to_string(
        serde_json::from_str::<serde_json::Value>(j).unwrap()
        .as_array().unwrap(),
        &Config::new().max_width(w))
    .unwrap()
}


#[test]
fn int() {
    assert_eq!(test_format(&[Value::Int64(10)]).unwrap(), "{10}");
    assert_eq!(test_format(&[
        Value::Int64(10),
        Value::Int64(20),
    ]).unwrap(), "{10, 20}");
}

#[test]
fn bigdecimal() {
    assert_eq!(test_format(&[
        Value::Decimal(TryFrom::try_from(
            BigDecimal::from_str("10.1").unwrap()
        ).unwrap()),
    ]).unwrap(), "{10.1n}");
}

#[test]
fn bigint() {
    assert_eq!(test_format(&[
        Value::BigInt(10.into()),
        Value::BigInt(10000.into()),
        Value::BigInt(100000000000i64.into()),
    ]).unwrap(), "{10n, 10000n, 1e11n}");
}

#[test]
fn decimal() {
    assert_eq!(test_format(&[
        Value::Decimal(TryFrom::try_from(
            BigDecimal::from_str("10e3").unwrap()
        ).unwrap()),
        Value::Decimal(TryFrom::try_from(
            BigDecimal::from_str("10e10").unwrap()
        ).unwrap()),
        Value::Decimal(TryFrom::try_from(
            BigDecimal::from_str("100000000000.1").unwrap()
        ).unwrap()),
        Value::Decimal(TryFrom::try_from(
            BigDecimal::from_str("0.000000000000508").unwrap()
        ).unwrap()),
    ]).unwrap(), "{10000.0n, 1.0e11n, 100000000000.1n, 0.508e-12}");
}

#[test]
fn array_ellipsis() {
    assert_eq!(test_format(&[
        Value::Array(vec![
            Value::Int64(10),
            Value::Int64(20),
            Value::Int64(30),
        ]),
    ]).unwrap(), "{[10, 20, 30]}");
    assert_eq!(test_format_cfg(&[
        Value::Array(vec![
            Value::Int64(10),
            Value::Int64(20),
            Value::Int64(30),
        ]),
    ], Config::new().max_items(2)).unwrap(), "{[10, 20, ...]}");
    assert_eq!(test_format_cfg(&[
        Value::Array(vec![
            Value::Int64(10),
            Value::Int64(20),
            Value::Int64(30),
        ]),
    ], Config::new().max_items(2).max_width(10)).unwrap(), r###"{
  [
    10,
    20,
    ... (further results hidden \limit 2)
  ],
}"###);
    assert_eq!(test_format_cfg(&[
        Value::Array(vec![
            Value::Int64(10),
        ]),
    ], Config::new().max_items(2)).unwrap(), "{[10]}");
}

#[test]
fn set_ellipsis() {
    assert_eq!(test_format(&[
        Value::Set(vec![
            Value::Int64(10),
            Value::Int64(20),
            Value::Int64(30),
        ]),
    ]).unwrap(), "{{10, 20, 30}}");
    assert_eq!(test_format_cfg(&[
        Value::Set(vec![
            Value::Int64(10),
            Value::Int64(20),
            Value::Int64(30),
        ]),
    ], Config::new().max_items(2)).unwrap(), "{{10, 20, ...}}");
    assert_eq!(test_format_cfg(&[
        Value::Set(vec![
            Value::Int64(10),
        ]),
    ], Config::new().max_items(2)).unwrap(), "{{10}}");
}

#[test]
fn wrap() {
    assert_eq!(test_format_cfg(&[
        Value::Int64(10),
        Value::Int64(20),
    ], Config::new().max_width(10)).unwrap(), "{10, 20}");
    assert_eq!(test_format_cfg(&[
        Value::Int64(10),
        Value::Int64(20),
        Value::Int64(30),
    ], Config::new().max_width(10)).unwrap(), "{\n  10,\n  20,\n  30,\n}");
}

#[test]
fn object() {
    let shape = ObjectShape::new(vec![
        ShapeElement {
            flag_implicit: false,
            flag_link_property: false,
            flag_link: false,
            name: "field1".into(),
        },
        ShapeElement {
            flag_implicit: false,
            flag_link_property: false,
            flag_link: false,
            name: "field2".into(),
        }
    ]);
    assert_eq!(test_format_cfg(&[
        Value::Object { shape: shape.clone(), fields: vec![
            Some(Value::Int32(10)),
            Some(Value::Int32(20)),
        ]},
        Value::Object { shape: shape.clone(), fields: vec![
            Some(Value::Int32(30)),
            Some(Value::Int32(40)),
        ]},
    ], Config::new().max_width(60)).unwrap(), r###"{
  Object {field1: 10, field2: 20},
  Object {field1: 30, field2: 40},
}"###);
    assert_eq!(test_format_cfg(&[
        Value::Object { shape: shape.clone(), fields: vec![
            Some(Value::Int32(10)),
            Some(Value::Int32(20)),
        ]},
        Value::Object { shape: shape.clone(), fields: vec![
            Some(Value::Int32(30)),
            None,
        ]},
    ], Config::new().max_width(20)).unwrap(), r###"{
  Object {
    field1: 10,
    field2: 20,
  },
  Object {
    field1: 30,
    field2: {},
  },
}"###);
}


#[test]
fn link_property() {
    let shape = ObjectShape::new(vec![
        ShapeElement {
            flag_implicit: false,
            flag_link_property: false,
            flag_link: false,
            name: "field1".into(),
        },
        ShapeElement {
            flag_implicit: false,
            flag_link_property: true,
            flag_link: false,
            name: "field2".into(),
        }
    ]);
    assert_eq!(test_format_cfg(&[
        Value::Object { shape: shape.clone(), fields: vec![
            Some(Value::Int32(10)),
            Some(Value::Int32(20)),
        ]},
        Value::Object { shape: shape.clone(), fields: vec![
            Some(Value::Int32(30)),
            Some(Value::Int32(40)),
        ]},
    ], Config::new().max_width(60)).unwrap(), r###"{
  Object {field1: 10, @field2: 20},
  Object {field1: 30, @field2: 40},
}"###);
}

#[test]
fn str() {
    assert_eq!(
        test_format(&[Value::Str("hello".into())]).unwrap(),
        r#"{"hello"}"#);
    assert_eq!(
        test_format(&[Value::Str("a\nb".into())]).unwrap(),
        "{\"a\\nb\"}");
    assert_eq!(
        test_format_cfg(&[Value::Str("a\nb".into())],
                        Config::new().expand_strings(true)).unwrap(),
        "{\n  'a\nb',\n}");
}

#[test]
fn all_widths() {
    let shape = ObjectShape::new(vec![
        ShapeElement {
            flag_implicit: false,
            flag_link_property: false,
            flag_link: false,
            name: "field1".into(),
        },
    ]);
    for width in 0..100 {
        test_format_cfg(&[
            Value::Object { shape: shape.clone(), fields: vec![
                Some(Value::Str(
                    "Sint tempor. Qui occaecat eu consectetur elit.".into())),
            ]},
        ], Config::new().max_width(width)).unwrap();
    }
}

#[test]
fn all_widths_json() {
    for width in 0..100 {
        json_fmt_width(width, r###"[
            {"field1": "Sint tempor. Qui occaecat eu consectetur elit."},
            {"field2": "Lorem ipsum dolor sit amet."}
        ]"###);
    }
}

#[test]
fn all_widths_json_item() {
    for width in 0..100 {
        json_fmt_width(width, r###"[
            {"field1": "Sint tempor. Qui occaecat eu consectetur elit."},
            {"field2": "Lorem ipsum dolor sit amet."}
        ]"###);
    }
}

#[test]
fn json() {
    assert_eq!(json_fmt("[10]"), "[10]");
    assert_eq!(json_fmt_width(20, r###"[
        {"field1": [],
         "field2": {}},
        {"field1": ["x"],
         "field2": {"a": 1}}
    ]
    "###), r###"[
  {
    "field1": [],
    "field2": {}
  },
  {
    "field1": ["x"],
    "field2": {
      "a": 1
    }
  }
]"###);
}
