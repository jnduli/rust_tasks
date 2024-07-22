# Local First Approach

I'd like to use `rt` locally without the need to ping the server, but also
occasionally get updates and syncs from both. A simpler approach involves using
the `modified_utc` key from the server to sync tasks locally and occassionally
bring everything down locally.

## Solution

We gave two rt instances A and B that were last synced n days ago.

If we have the last synced date, we can do:

- select all tasks from A with modified_utc > last_synced
- select all tasks from B with modified_utc > last_synced
- attempt to merge these two tasks using the modified_utc data.

By default, we can assumed last n days to sync things up, and this can be
provided as a parameter and we can store the current sync time locally too.


Workflow:

- Check if there's a local sync date stored somewhere.
- If there isn't, assume last n dates else use this date, call it sync_date.
- Get all tasks in server A that were updated after sync_date
- Get all tasks in server B that were updated after sync_date
- For each task in A and B:
	- if modified_utcs are different, update the task that has an earlier
	  modified_utc. Use ulids to get the same tasks.
	- if tasks don't exist, create tasks on server that lacks the task with
	  matching parameters.
- TODO: ensure modified_utc is set appropriately for each task btw.
- TODO: should support for sync daemon exist? Create rough template for this.

API:

```
rt sync n_days
rt sync --daemon
```

Toml configuration:

```
syncs:
[[store]]
strain = "Api"
uri = "http://...."

[[store]]
strain = SQLite"
uri = "file:///...."

[[store]]
strain = "Api":
uri = "http://...."
```





