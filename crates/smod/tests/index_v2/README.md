# v2 registry index test fixtures

`two-ports-index.json` is the golden fixture from `sunrise-org/sunrise-mods` PR #80
(`bot/tests/fixtures/two-ports/index.json`). The derived fixtures and `.minisig` files
were signed with freshly generated disposable Ed25519 keys (`test.pub` and `other.pub`)
whose private keys were deleted immediately after generation.

`first-published.json` and its `.minisig` are the first index the registry published
(`https://sunrise-org.github.io/sunrise-mods/v2/index.json`, serial 1795408124), signed with the
production key; the launcher's `the_production_key_verifies_the_first_published_index` checks them.
