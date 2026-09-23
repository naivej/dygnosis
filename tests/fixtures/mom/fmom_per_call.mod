// inventory: fmom_per_call
// call where a period belongs
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs;
var y; varexo e; periods y(1); values 1;
end;
