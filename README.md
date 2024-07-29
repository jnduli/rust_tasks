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

Summary configuration is optional and looks like:

```
[summary]
start = "08:00"
end = "17:00"
tags.meeting = "PT30M"
tags.work = "PT45M"
goal = "PT30M"
```

which we use to calculated some stats about how the day is going on.

Sync configuration is optinal and is similar to the `[backend]` config like:

```
[[sync]] # first sync
strain = "Api"
uri = "http://abc.co"

[[sync]] # second sync
strain = "SQLite"
uri = "file:///path/to/sync.db"
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
- [ ] explore using crdts as a storage type
