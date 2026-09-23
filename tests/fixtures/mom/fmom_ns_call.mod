// inventory: fmom_ns_call
// dotted moment call
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_moments;
foo.bar(y);
end;
