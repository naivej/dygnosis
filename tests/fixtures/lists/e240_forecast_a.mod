// inventory: lists_e240_forecast_a
// `forecast` takes endogenous variables only; `a` is a parameter.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

forecast a;
