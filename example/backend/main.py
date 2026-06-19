from fastapi import FastAPI

app = FastAPI()


class PersonService:
    def person(self):
        return {"name": "Ada", "age": 29, "mood": "Focused"}


@app.get("/api/person")
def get_person():
    service = PersonService()
    return service.person()
