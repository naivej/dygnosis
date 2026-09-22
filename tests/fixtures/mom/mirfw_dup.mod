// inventory: mom_e389_dup_tuple
// One `matched_irfs_weights` block listing the same six-part tuple twice. The
// weight differs; the tuple is what 7.1 keys on.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

matched_irfs_weights;
y(1), e, c(2), e, 0.5;
y(1), e, c(2), e, 0.7;
end;
