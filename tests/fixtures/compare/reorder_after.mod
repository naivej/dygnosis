// compare: unique names, reordered
var y r;
varexo e;
parameters beta rho;
beta = 0.99;
rho = 0.9;
model;
[name='taylor'] r = rho*r(-1) + e;
[name='euler'] y = beta*y(+1);
end;
