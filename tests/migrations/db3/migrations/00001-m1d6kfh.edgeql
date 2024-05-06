CREATE MIGRATION m1d6kfhjnqmrw4lleqvx6fibf5hpmndpw2tn2f6o4wm6fjyf55dhcq
    ONTO initial
{
  CREATE TYPE default::Child;
  CREATE TYPE default::Base {
      CREATE LINK foo -> default::Child;
  };
  CREATE TYPE default::Child2;
};
