// inventory: mom_e386_not_endogenous
// A `matched_moments` row naming a declared non-endogenous variable. 7.1's walk
// refuses: `Variable e is not an endogenous`.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

matched_moments;
e;
end;
