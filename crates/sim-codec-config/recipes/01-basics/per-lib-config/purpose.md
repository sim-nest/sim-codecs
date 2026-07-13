# Config codec descriptor

The config codec reads small configuration files into map values. A per-library
file decodes as one table, and a shared file decodes as a directory whose keys
are library ids and whose values are tables.
