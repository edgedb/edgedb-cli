CREATE MIGRATION m16ez5xxhkq3eiv6ebyi32igvokf2eiu4ks5wpr3emrweebmgqjqgq
    ONTO initial
{
  CREATE TYPE default::A {
      CREATE PROPERTY a -> std::str;
  };
  CREATE TYPE default::B {
      CREATE PROPERTY b -> std::int32;
  };
};
