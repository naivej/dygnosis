// inventory: fmom_per_beside
// integer beside a date
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs;
var y; varexo e; periods 1 2000Q1; values 1 2;
end;
