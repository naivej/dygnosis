// inventory: fmom_kw_weight_name
// weight that is a name
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs;
var y; varexo e; periods 1; values 1; weights y;
end;
