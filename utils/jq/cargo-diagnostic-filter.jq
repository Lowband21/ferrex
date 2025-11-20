fromjson?
| select(.reason == "compiler-message")
| .message as $m
| ($m.spans | map(select(.is_primary)) | first) as $span
| ($m.children | map(select(.level == "help")) | map(.message) | first) as $help
| ($m.spans
    | map(
        select(.suggested_replacement != null)
        | {replacement: .suggested_replacement, applicability: .suggestion_applicability}
      )
    | first) as $fix
| [
    $m.level,
    ($m.code.code // "unknown"),
    (if $span then "\($span.file_name):\($span.line_start):\($span.column_start)" else "" end),
    $m.message,
    ($help // ""),
    ($fix.replacement // ""),
    ($fix.applicability // "")
  ]
| @tsv
