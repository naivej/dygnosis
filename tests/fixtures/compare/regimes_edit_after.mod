// compare: repeated name, one regime text edited
var r;
parameters rho;
rho = 0.9;
model;
[name='policy', bind='ELB'] r = 0.1;
[name='policy', relax='ELB'] r = rho*r(-1);
end;
