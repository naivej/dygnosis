// inventory: e001_macro_decl_inactive_illegal
@#define use_bad = 0
var y
@#if use_bad
bad-name
@#else
x
@#endif
;
model;
y = 0;
x = 0;
end;
