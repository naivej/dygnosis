// compare: reorder equations, keep parameter and shock diffs
var y r;
varexo e;
parameters beta;
beta = 0.99;
model;
[name='euler'] y = beta*y(+1);
[name='taylor'] r = e;
end;
shocks;
var e; periods 2; values 1;
end;
