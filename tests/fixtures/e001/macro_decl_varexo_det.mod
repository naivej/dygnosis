// inventory: e001_macro_decl_varexo_det
var y;
varexo_det g
@#if 0
bad-name
@#else
tau
@#endif
;
predetermined_variables y
@#if 1
@#endif
;
model;
y = y(-1) + g + tau;
end;
