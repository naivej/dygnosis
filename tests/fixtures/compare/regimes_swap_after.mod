// compare: repeated name, regimes reordered, same text and tags
var r;
parameters rho;
rho = 0.9;
model;
[name='policy', relax='ELB'] r = rho*r(-1);
[name='policy', bind='ELB'] r = 0;
end;
