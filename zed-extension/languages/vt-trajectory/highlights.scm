(pair
  key: (string) @property
  value: (string) @string)

(pair
  key: (string) @keyword
  (#match? @keyword "\"(step|tool|mode|status)\"")
  value: (_))

(pair
  key: (string) @number
  value: (number) @number)
