// inventory: e001_macro_decl_active_illegal
@#define use_bad = 1
var y
@#if use_bad
bad-name
@#endif
;
model;
y = 0;
end;
