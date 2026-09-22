// inventory: lists_quiet_osr_aux_expect
// 7.1 warns about `AUX_EXPECT_1` and then aborts (exit 0xC0000409). The
// warning is skip-rewrite W186, so we stay silent.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

osr_params AUX_EXPECT_1;
