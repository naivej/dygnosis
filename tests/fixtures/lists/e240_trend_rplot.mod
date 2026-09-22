// inventory: lists_e240_trend_rplot
// `tt` is a `trend_var` name, which is in their table but is no endogenous.
trend_var(growth_factor=0.5) tt;
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

rplot tt;
