# Fuzz seed corpus for `fuzz_ssh_config`

Hand-crafted seed inputs that exercise ssh config features added after the
initial fuzz harness was written in v2.8.1. Libfuzzer mutates from this
corpus, so good seeds dramatically accelerate coverage of the new write
paths.

Covered features:

- `01_vault_ssh_basic.conf` — `# purple:vault-ssh` comment + `CertificateFile`
- `02_match_block_with_cert.conf` — Match block with its own CertificateFile
- `03_include_chain.conf` — multiple `Include` directives with glob
- `04_provider_metadata.conf` — `# purple:provider`, `provider_tags`, `meta`,
  `stale`, `tags`, `vault-ssh` all on one host
- `05_pattern_and_wildcard.conf` — `Host *`, `Host *.example.com`, concrete
- `06_crlf_and_tabs.conf` — tab indentation, `=` separator, inline comments,
  unknown directives
- `07_crlf_line_endings.conf` — real CRLF (written as bytes, do not edit in
  an LF-normalizing editor)
- `08_empty_and_comments.conf` — global comments, blank separators, single
  host

## Running the fuzzer

```bash
# One-time: copy seeds into the libfuzzer corpus directory.
mkdir -p fuzz/corpus/fuzz_ssh_config
cp fuzz/seed_corpus/fuzz_ssh_config/*.conf fuzz/corpus/fuzz_ssh_config/

# Run for 5 minutes (adjust -max_total_time).
cargo +nightly fuzz run fuzz_ssh_config -- -max_total_time=300
```

The `fuzz/corpus/` directory is gitignored. The seed corpus in this
directory is committed so every contributor starts from the same baseline.
