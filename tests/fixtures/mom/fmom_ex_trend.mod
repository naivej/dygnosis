// inventory: fmom_ex_trend
// trend variable in values
var y c;
varexo e;
varexo_det tau;
parameters a;
trend_var(growth_factor=1.01) A;
external_function(name=helper);
a = 0.5;

model;
y = a*y(-1) + e + A;
c = y;
#loc = y;
end;
matched_irfs;
var y; varexo e; periods 1; values (A);
end;
