// inventory: fmom_per_three
// three-part period range
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs;
var y; varexo e; periods 1:2:3; values 1;
end;
