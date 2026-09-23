// inventory: fmom_bad_mm
// bad moment row beside a stored one
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_moments;
y;
y = 3;
end;
