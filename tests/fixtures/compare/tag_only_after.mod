// compare: unique name, tag-only edit
var y;
parameters beta;
beta = 0.99;
model;
[name='euler', bind='ELB'] y = beta*y(+1);
end;
