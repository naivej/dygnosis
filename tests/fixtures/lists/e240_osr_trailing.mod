// inventory: lists_e240_osr_trailing
// The `osr` statement carries a list of its own, `{endogenous}`, told apart
// from the `osr_params` statement above it by the command value.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

osr_params a;
osr a;
