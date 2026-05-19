FROM pyd4vinci/scrapling:latest

WORKDIR /app

# Copy project files
COPY pyproject.toml .
COPY app/ app/
COPY run.py .
COPY debug_check.py .

# Install dependencies
RUN pip install --no-cache-dir fastapi "uvicorn[standard]" python-dotenv twitchio httpx curl_cffi

EXPOSE 9622

ENTRYPOINT ["python"]
CMD ["run.py"]
