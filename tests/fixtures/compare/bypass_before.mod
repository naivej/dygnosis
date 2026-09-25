// compare: repeated name on one side only
var r;
parameters rho;
rho = 0.9;
model;
[name='policy', bind='ELB'] r = 0;
[name='policy', relax='ELB'] r = rho*r(-1);
end;
