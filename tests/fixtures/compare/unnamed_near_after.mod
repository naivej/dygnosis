// compare: unnamed near-match to a named row
var y;
parameters beta;
beta = 0.99;
model;
[name='euler'] y = beta*y(+1) + y;
end;
