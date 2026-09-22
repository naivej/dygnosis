// inventory: lists_e240_plotsd_exo
// An exogenous is in neither arm of `{endogenous, epilogue}`.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

plot_shock_decomposition e;
