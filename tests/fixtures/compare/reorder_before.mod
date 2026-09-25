// compare: unique names, reordered
var y r;
varexo e;
parameters beta rho;
beta = 0.99;
rho = 0.9;
model;
[name='euler'] y = beta*y(+1);
[name='taylor'] r = rho*r(-1) + e;
end;
