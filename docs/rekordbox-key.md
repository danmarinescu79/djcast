# rekordbox database key

rekordbox 6/7 stores its library in `master.db`, a SQLite database encrypted with SQLCipher.
djcast reads the play history from it to detect the current track. To do that it needs the
database key.

**djcast does not ship this key.** You provide it once; it is stored only on your machine:

- Windows: `%APPDATA%\djcast\rekordbox-db.key`
- macOS: `~/Library/Application Support/djcast/rekordbox-db.key`

```sh
djcast rekordbox set-key <KEY>   # saves the key and checks that it unlocks master.db
djcast rekordbox info            # shows where master.db and the key file are
djcast rekordbox history         # prints your recent play history
```

You can also set `rekordbox_key` in the `[nowplaying]` section of `djcast.toml`.

## Obtaining the key

The key is the same for every rekordbox 6/7 installation. Ways to obtain it:

1. **From pyrekordbox**, if you have Python:
   ```sh
   pip install pyrekordbox
   python -c "from pyrekordbox.utils import deobfuscate; from pyrekordbox.masterdb.database import BLOB; print(deobfuscate(BLOB))"
   ```
2. **Extract it yourself** from your own rekordbox installation with a debugger. See the
   [pyrekordbox key guide](https://pyrekordbox.readthedocs.io/en/latest/key.html) for
   Windows (x64dbg) and macOS instructions.

## How djcast uses it
- `master.db` and its `-wal` file are opened **read-only** and decrypted **in memory**. No
  plaintext copy is written to disk, and rekordbox's files are never modified.
- The location of `master.db` is read from rekordbox's `rekordboxAgent/storage/options.json`
  (`db-path`), falling back to the default location.
- djcast never reads `rekordbox3.settings`, which also contains your account tokens.
