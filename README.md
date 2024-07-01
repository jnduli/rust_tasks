# Rust Tasks

A [tasklite](https://github.com/ad-si/TaskLite) clone made to learn rust. It has
some changes to cater for how I'd like to use the tool.

## Set up

### rust_tasks

The cli tool. Install with:

```
cargo install --path rust_tasks
```

and configure with a file in `$HOME/.config/rust_tasks/config.toml` like:

```
[backend]
strain = "Api"
uri = "http://127.0.0.1:3000"
```
or

```
[backend]
strain = "SQLite"
uri = "file:///path/to/sqlite.db"
```

Run:

```
rust_tasks --help
```

### Tasks Server

A server that's compatible with the cli tool's `Api` strain. Install with:

```
cargo install --path tasks_server
```

and configure with a file at `$HOME/.config/rust_tasks/tasks_server.toml` like:

```
db_uri = "file:///path/to/sqlite.db"
bind_address = "127.0.0.1:3000"
```

Or you can use docker by running:

```
docker image build -f server_dockerfile -t tasks_server .
docker container run --rm \
		-v $(pwd)/test_server.toml:/.config/rust_tasks/tasks_server.toml \
		-v /home/rookie/gdrive_rclone/tasklite/main.db:/tmp/tasklite.db \
		-u $(id -u):$(id -g) \
		-p 0.0.0.0:3000:3000 tasks_server
```

## Quirks

In guix, to install `rust_tasks`:

```
export CC=$(which gcc)
guix install sqlite
```

## TODO

- [ ] add support for `rt health` to check if storage is healthy
- [ ] add configurable `rt summary` docs
- [ ] explore using crdts as a storage type


## RT summary spec

- set start and end times per day
- set tags and approximate times for each
- set goal times I want

e.g.

[[summary]]
start: 08:00
end: 17:00
tags.meeting: 30m
tags.work: 60m
goal: 30m


