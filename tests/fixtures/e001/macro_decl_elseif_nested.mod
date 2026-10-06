// inventory: e001_macro_decl_elseif_nested
var y
@#if 0
a
@#elseif 0
bad-name
@#elseif 1
@#if 0
bad-name
@#else
x
@#endif
@#endif
;
varexo e
@#if 1
u
@#endif
;
parameters beta
@#if 0
bad-name
@#else
alpha
@#endif
;
beta = 0.99;
alpha = 0.33;
model;
y = beta * y(+1) + alpha * x + e + u;
x = 0;
end;
