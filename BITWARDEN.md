# Bitwarden enrollment and managed names

Select Bitwarden as a host password source and leave the value as `bw:` to opt into enrollment. On the first interactive connection, Purple unlocks the CLI if needed, asks for the server login password locally, and creates an item named `Purple / <alias>` in `ssh/Purple`. After acknowledged creation and verified readback, it binds the item UUID in the SSH config. Existing `bw:<UUID>` and legacy `bw:item-name`/search sources keep their behavior. Only UUID bindings participate in managed metadata updates.

Enrollment stores the password before SSH authentication, as the existing Keychain path does; it does not claim that the server accepted it. The first Bitwarden account login still uses `bw login`. Background tasks never prompt for enrollment. Moving a host from Keychain to Bitwarden preserves the original Keychain record.

Entries carry a `purple_host_identity` ownership field. Same-name external entries are not overwritten. An ambiguous create is recorded with identity/ID metadata only, then recovered by sync and lookup before another create is considered. No server password is stored in the recovery journal, SSH config, logs, or terminal output.

Changing the alias, hostname, username or port of a bound host queues an item-name and ownership-marker update. UUIDs, credentials and other item fields are preserved. Failed rename attempts retry on the next interactive connection. Items without the ownership marker, mismatched identities, and manually edited vault names are left untouched. This feature uses the existing `bw` CLI integration and requires no new Rust dependency.

Only personal-vault items may be enrolled or edited by the managed flow. Organization-owned items are left untouched. Newly created items must read back the exact entered password before UUID binding.

## Recovering an interrupted enrollment

An error shows the exact metadata journal path (under the Purple state directory's `bitwarden-enrollment/`). Do not blindly retry a create or delete vault items. Run `bw sync`, then inspect the personal vault and trash for the intended item and ownership marker. If it exists, bind its UUID to the host. Only after confirming no item was created, and with no enrollment process running, move that one journal file to a name ending in `.reviewed-backup` and retry. Keep the backup until recovery is verified. The same review is required if the host identity changed while enrollment was interrupted.

Rename errors identify the pending metadata file under `bitwarden-renames/`. Verify the UUID, ownership and current vault name. If the vault name or binding was deliberately changed by hand, close Purple and move that one pending file to `.reviewed-backup` to stop retrying. This changes neither the vault credentials nor the host's UUID binding.
