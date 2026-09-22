// inventory: mom_matched_irfs_weights_row
// Original AR(1) plus a definition. One `matched_irfs_weights` row: a four-name
// tuple, each endogenous with its own period, and one weight expression.
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
end;
