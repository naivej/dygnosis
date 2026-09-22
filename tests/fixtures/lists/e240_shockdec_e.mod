// inventory: lists_e240_shockdec_e
// `shock_decomposition` takes endogenous only; `e` is an exogenous.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

shock_decomposition e;
