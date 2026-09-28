// inventory: a range bound with arithmetic binds tighter than the colon
@#define N = 3
var y_1 y_2;
model;
@#for i in 1:N-1
y_@{i} = 0;
@#endfor
end;
