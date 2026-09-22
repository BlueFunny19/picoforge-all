# UI catalogs

`en_US.json` is the English source catalog; `zh_CN.json` contains Simplified Chinese translations. Keys are stable English source phrases. Use `i18n::tr` for static labels and `i18n::format` for numbered placeholders such as `{0}`. Format values separately; user content, credentials, protocol identifiers, and raw diagnostic output are never translated.

Keep terminology consistent: passkey → 通行密钥, security key → 安全密钥, object → 对象, slot → 槽位, security officer → 安全管理员, attestation → 认证. Chinese messages omit sentence-ending periods; use a semicolon between sentences where needed. Keep punctuation within file names, versions, URLs, and placeholders intact.

Use `LocalizedPlaceholder` for persistent input placeholders. Dropdown items store canonical source labels and translate during rendering. A language change preserves existing page state and background operations.

`cargo test --offline` checks catalog keys, placeholders, and Chinese sentence punctuation. Backend diagnostic strings are translated at the presentation boundary; raw tool output remains verbatim.
