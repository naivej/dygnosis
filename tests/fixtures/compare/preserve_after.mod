// compare: reorder equations, keep parameter and shock diffs
var y r;
varexo e;
parameters beta;
beta = 0.95;
model;
[name='taylor'] r = e;
[name='euler'] y = beta*y(+1);
end;
shocks;
var e; periods 3; values 1;
end;
