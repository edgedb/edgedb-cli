CREATE MIGRATION m12udjjofxzy3nygel35cq4tbz3v56vw7w3d3co6h5hmqhcnodqv3a
    ONTO m12bulrbounwj3oj5xsspa7gj676azrog6ndi45iyuwrwzvawkxraa
{
  CREATE TYPE default::Type2 {
      CREATE OPTIONAL SINGLE PROPERTY field2 -> std::str;
  };
};
