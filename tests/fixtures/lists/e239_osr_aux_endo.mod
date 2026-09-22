// inventory: lists_e239_osr_aux_endo
// `AUX_ENDO_1` is undeclared, and `osr_params` allows parameters only: the
// aux regex carries `AUX_EXPECT_` and `MULT_` for this set, so 7.1 refuses.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

osr_params AUX_ENDO_1;
