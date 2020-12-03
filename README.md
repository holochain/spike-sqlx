# spike-rust-sqlcipher-test

[Video Demo / Walkthrough (4:24)](https://www.youtube.com/watch?v=_pDPXtK0AmM)

### Setup

`rusqlite` doesn't yet include a bundled sqlcipher lib, so for now we need it to exist on the host system.

```shell
sudo apt-get install libsqlcipher-dev sqlcipher
```

### Run

```shell
cargo run
```

This will create an encrypted database (the key is 32 bytes zeroed), write one entry, then run an all-encompasing query printing the results.

### External tools

the sqlcipher command-line tool allows us to inspect / manipulate the database.

Run

```shell
./dump.bash
```

This will print out the schema and the entries table along with a bunch of statsfrom the database.
