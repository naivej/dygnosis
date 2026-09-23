// inventory: fmom_per_exp
// scientific period
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs;
var y; varexo e; periods 1e3; values 1;
end;
