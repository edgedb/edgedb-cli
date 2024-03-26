module default {
type Foo {
  required title: str;
  required year: int64;
  index on ((.year, .title));
}
}