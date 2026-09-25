// compare: different names, identical text
var y;
parameters beta;
beta = 0.99;
model;
[name='consumption'] y = beta*y(+1);
end;
