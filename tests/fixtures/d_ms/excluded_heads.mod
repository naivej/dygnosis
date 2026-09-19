// inventory: d_ms_excluded_heads
// A dotted head 7.1 sends to native MATLAB because of the pin's own exclusion:
// a mod-file local (`#x = 1;`) and an `external_function` name. Both statements
// are 7.1-accepted; neither may produce an Error here.
var y c k;
varexo e;
parameters alpha beta;
alpha = 0.36;
beta = 0.99;
model;
c = alpha*y + beta*c(-1) + e;
y = beta*y(-1) + c;
k = y;
end;
initval;
y = 0;
c = 0;
k = 0;
end;
shocks;
var e; stderr 0.1;
end;
#x = 1;
x.prior(shape=beta, mean=0.5, mean=0.6, stdev=0.1);
external_function(name=M_helper);
M_helper.prior(shape=beta, mean=0.5, mean=0.6);
