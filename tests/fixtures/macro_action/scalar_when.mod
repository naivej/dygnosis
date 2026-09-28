// inventory: a filtered scalar macro loop leaves two equations
@#define nums = 1:3
var y_1 y_3;
model;
@#for i in nums when i != 2
y_@{i} = 0;
@#endfor
end;
