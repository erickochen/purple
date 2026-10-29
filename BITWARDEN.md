# Bitwarden enrollment and managed names

Select Bitwarden as a host password source and leave the value as `bw:` to opt into enrollment. On the first interactive connection, Purple unlocks the CLI if needed, asks for the server login password locally, and creates an item named `Purple / <alias>` in `ssh/Purple`. After acknowledged creation and verified readback, it binds the item UUID in the SSH config. Existing `bw:<UUID>` sources keep their behavior.

Enrollment stores the password before SSH authentication, as the existing Keychain path does; it does not claim that the server accepted it. The first Bitwarden account login still uses `bw login`. Background tasks never prompt for enrollment. Moving a host from Keychain to Bitwarden preserves the original Keychain record.

Entries carry a `purple_host_identity` ownership field. Same-name external entries are not overwritten. An ambiguous create is recorded with identity/ID metadata only, then recovered by sync and lookup before another create is considered. No server password is stored in the recovery journal, SSH config, logs, or terminal output.

Renaming a bound host queues an item-name and ownership-marker update. UUIDs, credentials and other item fields are preserved. Failed rename attempts retry on the next interactive connection. Items without the ownership marker, mismatched identities, and manually edited vault names are left untouched. This feature uses the existing `bw` CLI integration and requires no new Rust dependency.
