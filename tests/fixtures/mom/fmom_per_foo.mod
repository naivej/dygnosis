// inventory: fmom_per_foo
// name where a period belongs
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs;
var y; varexo e; periods foo; values 1;
end;
