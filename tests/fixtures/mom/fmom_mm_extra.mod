// inventory: fmom_mm_extra
// paren on matched_moments
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_moments(extra);
y;
end;
